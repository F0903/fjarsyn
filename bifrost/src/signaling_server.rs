use std::{collections::HashMap, sync::Arc};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use fjarsyn_shared::{SignalingMessage, SignalingType};
use futures::{sink::SinkExt, stream::StreamExt};
use tokio::{
    net::{TcpListener, ToSocketAddrs},
    sync::{RwLock, mpsc},
};

#[derive(Debug)]
struct SignalingState {
    peers: HashMap<String, mpsc::Sender<SignalingMessage>>,
}

#[derive(Debug)]
pub struct SignalingServer {
    state: Arc<RwLock<SignalingState>>,
}

impl SignalingServer {
    pub fn new() -> Self {
        Self { state: Arc::new(RwLock::new(SignalingState { peers: HashMap::new() })) }
    }

    pub async fn listen(&self, listen_addr: impl ToSocketAddrs + std::fmt::Debug) {
        let router =
            Router::new().route("/ws", get(Self::ws_handler)).with_state(self.state.clone());
        tracing::info!("Signaling server listening on {:?}", listen_addr);

        let listener = TcpListener::bind(listen_addr).await.unwrap();
        axum::serve(listener, router).await.unwrap();
    }

    async fn ws_handler(
        ws: WebSocketUpgrade,
        State(state): State<Arc<RwLock<SignalingState>>>,
    ) -> impl IntoResponse {
        ws.on_upgrade(|socket| Self::handle_socket(socket, state))
    }

    async fn handle_socket(socket: WebSocket, state: Arc<RwLock<SignalingState>>) {
        // For simplicity, we'll use a random string to identify peers
        let peer_id = uuid::Uuid::new_v4().to_string();
        tracing::info!("New WebSocket connection with ID: {}", peer_id);

        // Split the socket into a sender and receiver.
        let (mut sender, mut receiver) = socket.split();

        // Create a new channel for this peer
        const PEER_MSG_BUF: usize = 100;
        let (tx, mut rx) = mpsc::channel(PEER_MSG_BUF);

        {
            let mut state = state.write().await;
            // Add the peer to the state
            state.peers.insert(peer_id.clone(), tx.clone());
        }

        // Send the identity message to the client
        let identity_msg = SignalingMessage {
            to: peer_id.clone(),
            from: "server".to_owned(),
            sig_type: SignalingType::Identity,
            data: peer_id.clone(),
        };
        if let Err(e) = tx.send(identity_msg).await {
            tracing::error!("Failed to send identity message: {}", e);
        }

        // This task will listen for messages on the channel and send them to the client
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match serde_json::to_string(&msg) {
                    Ok(json) => {
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => tracing::error!("Failed to serialize signaling message: {}", e),
                }
            }
        });

        // This loop will listen for messages from the client
        while let Some(Ok(msg)) = receiver.next().await {
            let Message::Text(text) = msg else {
                continue;
            };
            match serde_json::from_str::<SignalingMessage>(&text) {
                Ok(mut sig_msg) => {
                    // Overwrite the 'from' field with the actual peer ID to ensure authenticity
                    sig_msg.from = peer_id.clone();

                    let peers = {
                        let state = state.read().await;
                        state.peers.clone()
                    };

                    if sig_msg.to.is_empty() {
                        // Broadcast to all other peers
                        for (id, tx) in peers {
                            if id != peer_id {
                                let _ = tx.send(sig_msg.clone()).await;
                            }
                        }
                    } else {
                        // Send to specific peer
                        if let Some(tx) = peers.get(&sig_msg.to) {
                            let _ = tx.send(sig_msg).await;
                        } else {
                            tracing::warn!("Target peer {} not found", sig_msg.to);
                        }
                    }
                }
                Err(e) => tracing::error!("Failed to deserialize signaling message: {}", e),
            }
        }

        // Client disconnected, remove them from the state
        tracing::info!("Peer {} disconnected", peer_id);
        {
            let mut state = state.write().await;
            state.peers.remove(&peer_id);
        }
    }
}

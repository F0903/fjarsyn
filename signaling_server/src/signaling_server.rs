use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures::{sink::SinkExt, stream::StreamExt};
use loki_shared::{SignalingMessage, SignalingType};
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, ToSocketAddrs},
    sync::{Mutex, mpsc},
};

#[derive(Debug)]
struct SignalingState {
    peers: HashMap<String, mpsc::Sender<Message>>,
}

#[derive(Debug)]
pub struct SignalingServer {
    state: Arc<Mutex<SignalingState>>,
}

impl SignalingServer {
    pub fn new() -> Self {
        Self { state: Arc::new(Mutex::new(SignalingState { peers: HashMap::new() })) }
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
        State(state): State<Arc<Mutex<SignalingState>>>,
    ) -> impl IntoResponse {
        ws.on_upgrade(|socket| Self::handle_socket(socket, state))
    }

    async fn handle_socket(socket: WebSocket, state: Arc<Mutex<SignalingState>>) {
        // For simplicity, we'll use a random string to identify peers
        let peer_id = uuid::Uuid::new_v4().to_string();
        tracing::info!("New WebSocket connection with ID: {}", peer_id);

        // Split the socket into a sender and receiver.
        let (mut sender, mut receiver) = socket.split();

        // Create a new channel for this peer
        let (tx, mut rx) = mpsc::channel(100);

        let mut state = state.lock().await;

        // Add the peer to the state
        state.peers.insert(peer_id.clone(), tx);

        // This task will listen for messages on the channel and send them to the client
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sender.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // This loop will listen for messages from the client
        while let Some(Ok(msg)) = receiver.next().await {
            let peers = state.peers.clone();

            // Broadcast the message to all other peers
            for (id, tx) in peers {
                if id != peer_id {
                    let _ = tx.send(msg.clone()).await;
                }
            }
        }

        // Client disconnected, remove them from the state
        tracing::info!("Peer {} disconnected", peer_id);
        state.peers.remove(&peer_id);
    }
}

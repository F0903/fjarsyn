use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{RwLock, mpsc},
};
use tokio_tungstenite::{
    WebSocketStream, accept_async, connect_async, tungstenite::protocol::Message,
};

use crate::networking::{protocol::SignalingMessage, signaling_error::SignalingError};

type Result<T> = std::result::Result<T, SignalingError>;
type PeerRouteMap = Arc<RwLock<HashMap<String, (u64, mpsc::Sender<SignalingMessage>)>>>;

/// Connects directly to a peer's signaling listener.
pub async fn dial(
    addr: SocketAddr,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) -> Result<mpsc::Sender<SignalingMessage>> {
    let url = format!("ws://{}/ws", addr);
    tracing::debug!("Connecting to signaling URL: {}", url);

    let (ws_stream, _) = connect_async(url).await.map_err(|e| {
        tracing::error!("Failed to connect to signaling at {}: {}", addr, e);
        SignalingError::ConnectionFailed(e)
    })?;

    let (to_peer_tx, to_peer_rx) = mpsc::channel::<SignalingMessage>(100);

    tokio::spawn(manage_dialer_connection(ws_stream, to_peer_rx, to_webrtc_tx));

    Ok(to_peer_tx)
}

/// Starts a signaling listener on the specified port.
pub async fn listen(
    port: u16,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) -> Result<(mpsc::Sender<SignalingMessage>, u16)> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        tracing::error!("Failed to bind signaling listener to {}: {}", addr, e);
        SignalingError::ConnectionFailed(e.into())
    })?;

    let bound_port = listener.local_addr().unwrap().port();

    let (to_peer_tx, mut to_peer_rx_source) = mpsc::channel::<SignalingMessage>(100);
    let peer_routes: PeerRouteMap = Arc::new(RwLock::new(HashMap::new()));
    let peer_routes_for_router = peer_routes.clone();

    // Forward outbound signaling to the currently known peer route.
    tokio::spawn(async move {
        while let Some(msg) = to_peer_rx_source.recv().await {
            route_signaling_message(&peer_routes_for_router, msg).await;
        }
    });

    let next_connection_id = Arc::new(AtomicU64::new(1));

    tokio::spawn(async move {
        tracing::info!("P2P Signaling listener active on 0.0.0.0:{}", bound_port);
        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let to_webrtc_tx = to_webrtc_tx.clone();
                    let peer_routes = peer_routes.clone();
                    let connection_id = next_connection_id.fetch_add(1, Ordering::Relaxed);
                    let (connection_tx, connection_rx) = mpsc::channel::<SignalingMessage>(100);

                    tokio::spawn(async move {
                        tracing::info!("New signaling connection from {}", peer_addr);
                        if let Ok(ws_stream) = accept_async(stream).await {
                            manage_listener_connection(
                                ws_stream,
                                connection_id,
                                connection_tx,
                                connection_rx,
                                peer_routes,
                                to_webrtc_tx,
                            )
                            .await;
                        } else {
                            tracing::error!("Failed to accept WebSocket from {}", peer_addr);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("TCP accept error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    });

    Ok((to_peer_tx, bound_port))
}

async fn manage_dialer_connection<S>(
    mut ws_stream: WebSocketStream<S>,
    mut to_peer_rx: mpsc::Receiver<SignalingMessage>,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    loop {
        tokio::select! {
            msg = to_peer_rx.recv() => {
                match msg {
                    Some(message) => {
                        if send_signaling_message(&mut ws_stream, &message).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        tracing::debug!("Signaling channel dropped. Closing WebSocket dialer.");
                        let _ = ws_stream.close(None).await;
                        break;
                    }
                }
            }
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(signaling_message) =
                            serde_json::from_str::<SignalingMessage>(&text)
                            && to_webrtc_tx.send(signaling_message).await.is_err()
                        {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::debug!("WebSocket dialer closed by remote.");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::debug!("WebSocket dialer read error: {}", e);
                        break;
                    }
                    _ => {} // Ignore ping/pong/binary
                }
            }
        }
    }
}

async fn manage_listener_connection<S>(
    mut ws_stream: WebSocketStream<S>,
    connection_id: u64,
    connection_tx: mpsc::Sender<SignalingMessage>,
    mut to_peer_rx: mpsc::Receiver<SignalingMessage>,
    peer_routes: PeerRouteMap,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut registered_peer_id: Option<String> = None;

    loop {
        tokio::select! {
            msg = to_peer_rx.recv() => {
                match msg {
                    Some(message) => {
                        if send_signaling_message(&mut ws_stream, &message).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        tracing::debug!("Signaling connection channel closed. Closing WebSocket listener.");
                        let _ = ws_stream.close(None).await;
                        break;
                    }
                }
            }
            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(signaling_message) = serde_json::from_str::<SignalingMessage>(&text) {
                            register_peer_route(
                                &peer_routes,
                                connection_id,
                                connection_tx.clone(),
                                &signaling_message.from,
                                &mut registered_peer_id,
                            )
                            .await;

                            if to_webrtc_tx.send(signaling_message).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::debug!("WebSocket listener closed by remote.");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::debug!("WebSocket listener read error: {}", e);
                        break;
                    }
                    _ => {} // Ignore ping/pong/binary
                }
            }
        }
    }

    unregister_peer_route(&peer_routes, connection_id, registered_peer_id.as_deref()).await;
}

async fn route_signaling_message(peer_routes: &PeerRouteMap, message: SignalingMessage) {
    if let Some(peer_id) = message.to.clone() {
        let route = {
            let routes = peer_routes.read().await;
            routes.get(&peer_id).map(|(_, sender)| sender.clone())
        };

        if let Some(route) = route {
            let _ = route.send(message).await;
        } else {
            tracing::debug!("No signaling route found for peer {}", peer_id);
        }
        return;
    }

    let routes = {
        let routes = peer_routes.read().await;
        routes.values().map(|(_, sender)| sender.clone()).collect::<Vec<_>>()
    };

    for route in routes {
        let _ = route.send(message.clone()).await;
    }
}

async fn register_peer_route(
    peer_routes: &PeerRouteMap,
    connection_id: u64,
    connection_tx: mpsc::Sender<SignalingMessage>,
    peer_id: &str,
    registered_peer_id: &mut Option<String>,
) {
    let mut routes = peer_routes.write().await;
    routes.insert(peer_id.to_string(), (connection_id, connection_tx));
    *registered_peer_id = Some(peer_id.to_string());
}

async fn unregister_peer_route(
    peer_routes: &PeerRouteMap,
    connection_id: u64,
    peer_id: Option<&str>,
) {
    let Some(peer_id) = peer_id else {
        return;
    };

    let mut routes = peer_routes.write().await;
    if routes.get(peer_id).is_some_and(|(id, _)| *id == connection_id) {
        routes.remove(peer_id);
    }
}

async fn send_signaling_message<S>(
    ws_stream: &mut WebSocketStream<S>,
    msg: &SignalingMessage,
) -> std::result::Result<(), ()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    match serde_json::to_string(msg) {
        Ok(json) => {
            if ws_stream.send(Message::Text(json.into())).await.is_err() {
                return Err(());
            }
            Ok(())
        }
        Err(e) => {
            tracing::error!("Failed to serialize signaling message: {}", e);
            Err(())
        }
    }
}

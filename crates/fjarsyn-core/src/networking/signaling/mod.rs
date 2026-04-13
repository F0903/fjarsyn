mod dialer;
mod listener;
mod routing;
mod transport;

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio::{
    net::TcpListener,
    sync::{RwLock, mpsc},
};
use tokio_tungstenite::{accept_async, connect_async};

use crate::networking::{protocol::SignalingMessage, signaling_error::SignalingError};

type Result<T> = std::result::Result<T, SignalingError>;
pub(super) type PeerRouteMap = Arc<RwLock<HashMap<String, (u64, mpsc::Sender<SignalingMessage>)>>>;

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

    tokio::spawn(dialer::manage_dialer_connection(ws_stream, to_peer_rx, to_webrtc_tx));

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

    tokio::spawn(async move {
        while let Some(msg) = to_peer_rx_source.recv().await {
            routing::route_signaling_message(&peer_routes_for_router, msg).await;
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
                            listener::manage_listener_connection(
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

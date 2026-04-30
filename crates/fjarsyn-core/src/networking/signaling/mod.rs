pub mod auth;
mod dialer;
mod listener;
mod routing;
mod transport;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio::{net::TcpListener, sync::mpsc};
use tokio_tungstenite::{accept_async, connect_async};
pub(crate) use transport::SignalingAuthContext;

use crate::networking::{protocol::SignalingMessage, signaling_error::SignalingError};

type Result<T> = std::result::Result<T, SignalingError>;

/// Connects directly to a peer's signaling listener.
pub(crate) async fn dial(
    addr: SocketAddr,
    auth: SignalingAuthContext,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) -> Result<mpsc::Sender<SignalingMessage>> {
    let url = format!("ws://{}/ws", addr);
    tracing::debug!("Connecting to signaling URL: {}", url);

    let (ws_stream, _) = connect_async(url).await.map_err(|e| {
        tracing::error!("Failed to connect to signaling at {}: {}", addr, e);
        SignalingError::ConnectionFailed(e)
    })?;

    let (to_peer_tx, to_peer_rx) = mpsc::channel::<SignalingMessage>(100);

    tokio::spawn(dialer::manage_dialer_connection(ws_stream, auth, to_peer_rx, to_webrtc_tx));

    Ok(to_peer_tx)
}

/// Starts a signaling listener on the specified port.
pub(crate) async fn listen(
    port: u16,
    auth: SignalingAuthContext,
    local_peer_id: String,
    to_webrtc_tx: mpsc::Sender<SignalingMessage>,
) -> Result<(mpsc::Sender<SignalingMessage>, u16)> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        tracing::error!("Failed to bind signaling listener to {}: {}", addr, e);
        SignalingError::ConnectionFailed(e.into())
    })?;

    let bound_port = listener.local_addr().unwrap().port();

    let (to_peer_tx, mut to_peer_rx_source) = mpsc::channel::<SignalingMessage>(100);
    let peer_routes = Arc::new(routing::PeerRoutes::default());
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
                    let auth = auth.clone();
                    let local_peer_id = local_peer_id.clone();
                    let connection_id = next_connection_id.fetch_add(1, Ordering::Relaxed);
                    let (connection_tx, connection_rx) = mpsc::channel::<SignalingMessage>(100);

                    tokio::spawn(async move {
                        tracing::info!("New signaling connection from {}", peer_addr);
                        if let Ok(ws_stream) = accept_async(stream).await {
                            listener::manage_listener_connection(listener::ListenerConnection {
                                ws_stream,
                                connection_id,
                                auth,
                                local_peer_id,
                                connection_tx,
                                to_peer_rx: connection_rx,
                                peer_routes,
                                to_webrtc_tx,
                            })
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock as StdRwLock};

    use chrono::Utc;
    use futures_util::SinkExt;
    use tokio::{
        sync::mpsc,
        time::{Duration, timeout},
    };
    use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

    use super::*;
    use crate::networking::{
        protocol::{SignalingMessage, SignalingType},
        signaling::auth::{
            LocalPeerIdentity, SignedSignalingEnvelope, TrustedPeerDirectory, TrustedPeerIdentity,
        },
    };

    fn auth_context(
        local_identity: LocalPeerIdentity,
        trusted_peer: Option<(&str, &LocalPeerIdentity)>,
    ) -> SignalingAuthContext {
        let trusted_peers = trusted_peer
            .map(|(peer_id, identity)| {
                TrustedPeerDirectory::new([TrustedPeerIdentity::new(
                    peer_id,
                    identity.public_key_base64(),
                )])
            })
            .unwrap_or_default();

        SignalingAuthContext::new(local_identity, Arc::new(StdRwLock::new(trusted_peers)))
    }

    fn offer_from(peer_id: &str) -> SignalingMessage {
        SignalingMessage {
            from: peer_id.to_string(),
            to: Some("peer-a".into()),
            sig_type: SignalingType::Offer,
            data: "sdp".into(),
        }
    }

    async fn listener_socket(
        auth: SignalingAuthContext,
    ) -> (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        mpsc::Receiver<SignalingMessage>,
    ) {
        let (to_webrtc_tx, to_webrtc_rx) = mpsc::channel(4);
        let (_listener_tx, port) = listen(0, auth, "peer-a".into(), to_webrtc_tx).await.unwrap();
        let url = format!("ws://127.0.0.1:{port}/ws");
        let (ws_stream, _) = connect_async(url).await.unwrap();
        (ws_stream, to_webrtc_rx)
    }

    #[tokio::test]
    async fn listener_rejects_unsigned_signaling_messages_end_to_end() {
        let local_identity = LocalPeerIdentity::generate();
        let remote_identity = LocalPeerIdentity::generate();
        let auth = auth_context(local_identity, Some(("peer-b", &remote_identity)));
        let (mut ws_stream, mut to_webrtc_rx) = listener_socket(auth).await;
        let unsigned = serde_json::to_string(&offer_from("peer-b")).unwrap();

        ws_stream.send(Message::Text(unsigned.into())).await.unwrap();

        assert!(timeout(Duration::from_millis(100), to_webrtc_rx.recv()).await.is_err());
    }

    #[tokio::test]
    async fn listener_rejects_replayed_signed_messages_end_to_end() {
        let local_identity = LocalPeerIdentity::generate();
        let remote_identity = LocalPeerIdentity::generate();
        let auth = auth_context(local_identity, Some(("peer-b", &remote_identity)));
        let (mut ws_stream, mut to_webrtc_rx) = listener_socket(auth).await;
        let message = offer_from("peer-b");
        let envelope =
            SignedSignalingEnvelope::sign(&remote_identity, message.clone(), Utc::now()).unwrap();
        let signed = serde_json::to_string(&envelope).unwrap();

        ws_stream.send(Message::Text(signed.clone().into())).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(1), to_webrtc_rx.recv()).await.unwrap(),
            Some(message)
        );

        ws_stream.send(Message::Text(signed.into())).await.unwrap();
        assert!(timeout(Duration::from_millis(100), to_webrtc_rx.recv()).await.is_err());
    }
}

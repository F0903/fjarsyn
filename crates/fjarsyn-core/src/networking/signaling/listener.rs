use std::sync::Arc;

use futures_util::StreamExt;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message};

use super::{
    routing::{PeerRoutes, register_peer_route, unregister_peer_route},
    transport::{SignalingAuthContext, send_signaling_message, verify_incoming_signaling_message},
};
use crate::networking::protocol::SignalingMessage;

pub(super) struct ListenerConnection<S> {
    pub(super) ws_stream: WebSocketStream<S>,
    pub(super) connection_id: u64,
    pub(super) auth: SignalingAuthContext,
    pub(super) local_peer_id: String,
    pub(super) connection_tx: mpsc::Sender<SignalingMessage>,
    pub(super) to_peer_rx: mpsc::Receiver<SignalingMessage>,
    pub(super) peer_routes: Arc<PeerRoutes>,
    pub(super) to_webrtc_tx: mpsc::Sender<SignalingMessage>,
}

pub(super) async fn manage_listener_connection<S>(connection: ListenerConnection<S>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let ListenerConnection {
        mut ws_stream,
        connection_id,
        auth,
        local_peer_id,
        connection_tx,
        mut to_peer_rx,
        peer_routes,
        to_webrtc_tx,
    } = connection;
    let mut registered_peer_id: Option<String> = None;

    loop {
        tokio::select! {
            msg = to_peer_rx.recv() => {
                match msg {
                    Some(message) => {
                        if send_signaling_message(&mut ws_stream, &auth, &message).await.is_err() {
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
                        let signaling_message = match verify_incoming_signaling_message(&auth, &text) {
                            Ok(signaling_message) => signaling_message,
                            Err(err) => {
                                tracing::warn!("Rejected signed signaling message on listener: {}", err);
                                continue;
                            }
                        };

                        if !signaling_message.targets_peer(&local_peer_id) {
                            tracing::debug!(
                                "Ignoring signaling message from {} addressed to {:?}.",
                                signaling_message.from,
                                signaling_message.to
                            );
                            continue;
                        }

                        if !register_peer_route(
                            &peer_routes,
                            connection_id,
                            connection_tx.clone(),
                            &signaling_message.from,
                            &mut registered_peer_id,
                        )
                        .await
                        {
                            continue;
                        }

                        if to_webrtc_tx.send(signaling_message).await.is_err() {
                            break;
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
                    _ => {}
                }
            }
        }
    }

    unregister_peer_route(&peer_routes, connection_id, registered_peer_id.as_deref()).await;
}

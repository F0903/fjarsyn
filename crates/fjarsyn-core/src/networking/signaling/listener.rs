use futures_util::StreamExt;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message};

use super::{
    PeerRouteMap,
    routing::{register_peer_route, unregister_peer_route},
    transport::send_signaling_message,
};
use crate::networking::protocol::SignalingMessage;

pub(super) async fn manage_listener_connection<S>(
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
                    _ => {}
                }
            }
        }
    }

    unregister_peer_route(&peer_routes, connection_id, registered_peer_id.as_deref()).await;
}

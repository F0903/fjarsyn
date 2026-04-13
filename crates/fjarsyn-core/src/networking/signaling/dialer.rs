use futures_util::StreamExt;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message};

use super::transport::send_signaling_message;
use crate::networking::protocol::SignalingMessage;

pub(super) async fn manage_dialer_connection<S>(
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
                    _ => {}
                }
            }
        }
    }
}

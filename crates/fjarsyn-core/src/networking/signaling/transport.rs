use futures_util::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message};

use crate::networking::protocol::SignalingMessage;

pub(super) async fn send_signaling_message<S>(
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

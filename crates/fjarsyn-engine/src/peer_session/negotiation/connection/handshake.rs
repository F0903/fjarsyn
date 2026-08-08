use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::protocol::{Message, WebSocketConfig},
};

use crate::peer_session::{Error, negotiation::Limits, protocol::SignedSessionEnvelope};

pub(in crate::peer_session::negotiation) async fn send_handshake_envelope<S>(
    socket: &mut WebSocketStream<S>,
    envelope: SignedSessionEnvelope,
    limits: &Limits,
) -> Result<(), Error>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let serialized =
        serde_json::to_string(&envelope).map_err(|error| Error::Protocol(error.to_string()))?;
    if serialized.len() > limits.max_frame_bytes {
        return Err(Error::Protocol("signaling handshake frame exceeds size limit".into()));
    }
    tokio::time::timeout(limits.handshake_timeout, socket.send(Message::Text(serialized.into())))
        .await
        .map_err(|_| Error::Signaling("signaling handshake write timed out".into()))?
        .map_err(|error| Error::Signaling(error.to_string()))
}

pub(in crate::peer_session::negotiation) async fn receive_handshake_envelope<S>(
    socket: &mut WebSocketStream<S>,
    limits: &Limits,
) -> Result<SignedSessionEnvelope, Error>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(limits.handshake_timeout, socket.next())
        .await
        .map_err(|_| Error::Signaling("signaling handshake read timed out".into()))?
        .ok_or_else(|| Error::Signaling("signaling connection closed".into()))?
        .map_err(|error| Error::Signaling(error.to_string()))?;
    parse_envelope(message, limits.max_frame_bytes)
}

pub(in crate::peer_session::negotiation) fn websocket_config(limits: &Limits) -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(limits.max_frame_bytes))
        .max_frame_size(Some(limits.max_frame_bytes))
}

pub(super) fn parse_envelope(
    message: Message,
    max_frame_bytes: usize,
) -> Result<SignedSessionEnvelope, Error> {
    let Message::Text(text) = message else {
        return Err(Error::Protocol("signaling frames must be UTF-8 text".into()));
    };
    if text.len() > max_frame_bytes {
        return Err(Error::Protocol("signaling frame exceeds size limit".into()));
    }
    serde_json::from_str(&text).map_err(|error| Error::Protocol(error.to_string()))
}

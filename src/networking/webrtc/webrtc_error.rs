use crate::networking::signaling_error::SignalingError;

pub type WebRTCResult<T> = Result<T, WebRTCError>;

#[derive(thiserror::Error, Debug)]
pub enum WebRTCError {
    #[error("Peer connection error: {0}")]
    PeerConnectionError(webrtc::Error),
    #[error("Codec error: {0}")]
    CodecError(webrtc::Error),
    #[error("SDP error: {0}")]
    SdpError(webrtc::Error),
    #[error("Send error: {0}")]
    SendError(tokio::sync::mpsc::error::SendError<loki_shared::SignalingMessage>),
    #[error("Deserialize error: {0}")]
    DeserializeError(serde_json::Error),
    #[error("Signaling error: {0}")]
    SignalingError(#[from] SignalingError),
    #[error("Media error: {0}")]
    MediaError(#[from] webrtc::media::Error),
}

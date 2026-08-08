use crate::peer_session::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecDirection {
    Encoder,
    Decoder,
}

#[derive(Debug, Clone)]
pub enum Event {
    LocalFailed { session_id: SessionId, reason: String },
    RemoteFailed { session_id: SessionId, reason: String },
    CodecRestartRequired { direction: CodecDirection },
}

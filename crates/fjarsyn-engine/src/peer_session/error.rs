use super::SessionId;
use crate::identity::{PeerId, PeerIdError};

/// Errors exposed by the peer-session application boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    InvalidPeerId(#[from] PeerIdError),
    #[error("invalid {kind} identifier: {value}")]
    InvalidIdentifier { kind: &'static str, value: String },
    #[error("peer-session limit {name} must be greater than zero")]
    InvalidLimit { name: &'static str },
    #[error("peer {0} is not trusted")]
    PeerNotTrusted(PeerId),
    #[error("peer {0} is suspended while its trusted identity is being changed")]
    PeerSuspended(PeerId),
    #[error("peer {0} is not currently nearby")]
    PeerNotNearby(PeerId),
    #[error("all {attempted} signaling endpoint attempts for peer {peer_id} failed")]
    EndpointAttemptsExhausted { peer_id: PeerId, attempted: usize },
    #[error("a session with peer {0} already exists")]
    SessionAlreadyExists(PeerId),
    #[error("session {0} does not exist")]
    SessionNotFound(SessionId),
    #[error("session {0} is busy")]
    SessionBusy(SessionId),
    #[error("session {session_id} is in phase {phase}, which does not allow {operation}")]
    InvalidState { session_id: SessionId, phase: &'static str, operation: &'static str },
    #[error("message body cannot be empty")]
    EmptyMessage,
    #[error("message body exceeds the {max} byte limit")]
    MessageTooLarge { max: usize },
    #[error("screen share {0} is not the active local share")]
    ShareMismatch(super::ShareId),
    #[error("the peer-session service has stopped")]
    ServiceStopped,
    #[error("peer-session command response was dropped")]
    ResponseDropped,
    #[error("peer-session operation timed out")]
    OperationTimeout,
    #[error("peer-session operation was accepted but its final outcome is unknown")]
    OutcomeUnknown,
    #[error("signaling listener failed: {0}")]
    Listener(String),
    #[error("signaling connection failed: {0}")]
    Signaling(String),
    #[error("WebRTC operation failed: {0}")]
    WebRtc(String),
    #[error("peer-session protocol error: {0}")]
    Protocol(String),
    #[error("peer-session media input has closed")]
    MediaClosed,
    #[error("remote video consumer fell {skipped} samples behind")]
    RemoteVideoLagged { skipped: u64 },
    #[error("peer-session shutdown timed out")]
    ShutdownTimeout,
}

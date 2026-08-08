//! Peer-session UI actions and their asynchronous outcomes.

use fjarsyn_engine::{
    identity::PeerId, media::capture::PlatformItem, peer_session::SessionId,
    screen_share::Selection,
};

#[derive(Debug, Clone)]
pub(in crate::ui) enum SendOutcome {
    Sent,
    DeliveryUncertain,
    Failed(String),
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Action {
    Connect(PeerId),
    ConnectCompleted(Result<SessionId, String>),
    Accept { session_id: SessionId },
    Reject { session_id: SessionId },
    Disconnect { session_id: SessionId },
    SessionCommandCompleted(Result<(), String>),
    SendMessage { session_id: SessionId, peer_id: PeerId, body: String },
    MessageSent(SendOutcome),
    BeginScreenShare { session_id: SessionId },
    CaptureSourceSelected { selection: Selection, result: Result<Option<PlatformItem>, String> },
    StopScreenShare { session_id: SessionId },
    ScreenShareCompleted(Result<(), String>),
}

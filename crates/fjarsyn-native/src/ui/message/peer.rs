use fjarsyn_core::{
    capture_providers::PlatformCaptureItem,
    peer_session::{PeerId, SessionId},
};

#[derive(Debug, Clone)]
pub enum MessageSendOutcome {
    Sent,
    DeliveryUncertain,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum PeerActionMessage {
    Connect(PeerId),
    ConnectCompleted(Result<SessionId, String>),
    Accept {
        session_id: SessionId,
    },
    Reject {
        session_id: SessionId,
    },
    Disconnect {
        session_id: SessionId,
    },
    SessionCommandCompleted(Result<(), String>),
    SendMessage {
        session_id: SessionId,
        peer_id: PeerId,
        body: String,
    },
    MessageSent(MessageSendOutcome),
    BeginScreenShare {
        session_id: SessionId,
    },
    CaptureSourceSelected {
        session_id: SessionId,
        result: Result<Option<PlatformCaptureItem>, String>,
    },
    StopScreenShare {
        session_id: SessionId,
    },
    ScreenShareCompleted(Result<(), String>),
}

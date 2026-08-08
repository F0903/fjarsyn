use chrono::{DateTime, Utc};

use super::{CloseReason, LocalShareState, MessageId, RemoteShareState, SessionId};
use crate::identity::PeerId;

/// Semantic events emitted after peer and session authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    IncomingRequest {
        session_id: SessionId,
        peer_id: PeerId,
    },
    Connected {
        session_id: SessionId,
        peer_id: PeerId,
    },
    Closed {
        session_id: SessionId,
        peer_id: PeerId,
        reason: CloseReason,
    },
    MessageSent {
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    },
    MessageReceived {
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    },
    MessageReceiptReceived {
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    },
    LocalShareChanged {
        session_id: SessionId,
        peer_id: PeerId,
        state: LocalShareState,
    },
    RemoteShareChanged {
        session_id: SessionId,
        peer_id: PeerId,
        state: RemoteShareState,
    },
}

impl Event {
    pub fn session_id(&self) -> SessionId {
        match self {
            Self::IncomingRequest { session_id, .. }
            | Self::Connected { session_id, .. }
            | Self::Closed { session_id, .. }
            | Self::MessageSent { session_id, .. }
            | Self::MessageReceived { session_id, .. }
            | Self::MessageReceiptReceived { session_id, .. }
            | Self::LocalShareChanged { session_id, .. }
            | Self::RemoteShareChanged { session_id, .. } => *session_id,
        }
    }
}

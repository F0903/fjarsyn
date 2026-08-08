use super::MessageStatus;
use crate::{
    identity::PeerId,
    peer_session::{MessageId, SessionId},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ConversationUpdated {
        peer_id: PeerId,
    },
    IncomingMessage {
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        body: String,
    },
    MessageStatusChanged {
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        status: MessageStatus,
    },
}

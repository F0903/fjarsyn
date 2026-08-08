use super::{MessageRecordError, MessageStatus, StoreError};
use crate::{
    identity::PeerId,
    peer_session::{self, MessageId, SessionId},
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("peer session error: {0}")]
    Session(#[from] peer_session::Error),
    #[error(transparent)]
    MessageRecord(#[from] MessageRecordError),
    #[error("message body cannot be empty")]
    EmptyBody,
    #[error("message body exceeds the {max} byte limit")]
    MessageTooLarge { max: usize },
    #[error("session {session_id} belongs to {actual_peer}, not {expected_peer}")]
    SessionPeerMismatch { session_id: SessionId, expected_peer: PeerId, actual_peer: PeerId },
    #[error("session {session_id} is not connected")]
    SessionNotConnected { session_id: SessionId },
    #[error("message {message_id} could not transition to {status:?}")]
    MissingMessageTransition { message_id: MessageId, status: MessageStatus },
    #[error("the messaging service has stopped")]
    ServiceStopped,
    #[error("the messaging service command queue is full")]
    ServiceBusy,
    #[error("the messaging command expired before execution")]
    CommandExpired,
    #[error("the messaging service is stopping")]
    ServiceStopping,
    #[error("the messaging command response was dropped")]
    ResponseDropped,
    #[error("messaging shutdown timed out")]
    ShutdownTimeout,
    #[error("the messaging task failed: {0}")]
    TaskFailed(String),
}

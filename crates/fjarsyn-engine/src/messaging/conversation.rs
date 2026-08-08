//! Validated conversation messages and their read-model projections.

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};

use super::MessageRecord;
use crate::{
    identity::PeerId,
    peer_session::{MessageId, SessionId},
};

pub type ConversationMap = HashMap<PeerId, Arc<Vec<ConversationMessage>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    Incoming,
    Outgoing,
}

impl MessageDirection {
    fn from_db(value: &str) -> Result<Self, MessageRecordError> {
        match value {
            "incoming" => Ok(Self::Incoming),
            "outgoing" => Ok(Self::Outgoing),
            _ => Err(MessageRecordError::UnknownDirection(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    Pending,
    Sent,
    Delivered,
    /// The transport may have accepted the message, but no definitive send
    /// result or authenticated receipt was observed.
    Unknown,
    Failed,
}

impl MessageStatus {
    fn from_db(value: &str) -> Result<Self, MessageRecordError> {
        match value {
            "pending" => Ok(Self::Pending),
            "sent" => Ok(Self::Sent),
            "delivered" => Ok(Self::Delivered),
            "unknown" => Ok(Self::Unknown),
            "failed" => Ok(Self::Failed),
            _ => Err(MessageRecordError::UnknownStatus(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MessageRecordError {
    #[error("unknown message direction: {0}")]
    UnknownDirection(String),
    #[error("unknown message status: {0}")]
    UnknownStatus(String),
    #[error("invalid session ID in message record: {0}")]
    InvalidSessionId(String),
    #[error("invalid message ID in message record: {0}")]
    InvalidMessageId(String),
    #[error("invalid peer ID in message record: {0}")]
    InvalidPeerId(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    pub id: i64,
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub peer_id: PeerId,
    pub direction: MessageDirection,
    pub body: String,
    pub status: MessageStatus,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

impl TryFrom<MessageRecord> for ConversationMessage {
    type Error = MessageRecordError;

    fn try_from(value: MessageRecord) -> Result<Self, Self::Error> {
        let session_id = value
            .session_id
            .parse()
            .map_err(|_| MessageRecordError::InvalidSessionId(value.session_id.clone()))?;
        let message_id = value
            .message_id
            .parse()
            .map_err(|_| MessageRecordError::InvalidMessageId(value.message_id.clone()))?;
        let peer_id = PeerId::new(value.peer_id.clone())
            .map_err(|_| MessageRecordError::InvalidPeerId(value.peer_id.clone()))?;

        Ok(Self {
            id: value.id,
            session_id,
            message_id,
            peer_id,
            direction: MessageDirection::from_db(&value.direction)?,
            body: value.body,
            status: MessageStatus::from_db(&value.status)?,
            created_at: value.created_at,
            delivered_at: value.delivered_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSummary {
    pub peer_id: PeerId,
    pub last_message_row_id: i64,
    pub last_message_body: String,
    pub last_message_direction: MessageDirection,
    pub last_message_status: MessageStatus,
    pub last_message_at: DateTime<Utc>,
}

impl From<&ConversationMessage> for ConversationSummary {
    fn from(value: &ConversationMessage) -> Self {
        Self {
            peer_id: value.peer_id.clone(),
            last_message_row_id: value.id,
            last_message_body: value.body.clone(),
            last_message_direction: value.direction,
            last_message_status: value.status,
            last_message_at: value.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_persisted_identifiers_are_rejected() {
        let now = Utc::now();
        let model = MessageRecord {
            id: 1,
            session_id: "not-a-uuid".into(),
            message_id: MessageId::new().to_string(),
            peer_id: "peer-a".into(),
            direction: "incoming".into(),
            body: "hello".into(),
            status: "delivered".into(),
            created_at: now,
            delivered_at: Some(now),
        };

        assert!(matches!(
            ConversationMessage::try_from(model),
            Err(MessageRecordError::InvalidSessionId(_))
        ));
    }
}

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};

use crate::database::MessageModel;

pub type ConversationMap = HashMap<String, Arc<Vec<ConversationMessage>>>;

#[derive(Debug, Clone)]
pub enum MessagingEvent {
    ConversationUpdated { peer_id: String },
    IncomingMessage { peer_id: String, body: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    Incoming,
    Outgoing,
}

impl MessageDirection {
    pub fn from_db(value: &str) -> Result<Self, MessageRecordError> {
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
    Delivered,
    Failed,
}

impl MessageStatus {
    pub fn from_db(value: &str) -> Result<Self, MessageRecordError> {
        match value {
            "pending" => Ok(Self::Pending),
            "delivered" => Ok(Self::Delivered),
            "failed" => Ok(Self::Failed),
            _ => Err(MessageRecordError::UnknownStatus(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    pub id: i64,
    pub message_id: String,
    pub peer_id: String,
    pub direction: MessageDirection,
    pub body: String,
    pub status: MessageStatus,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

impl TryFrom<MessageModel> for ConversationMessage {
    type Error = MessageRecordError;

    fn try_from(value: MessageModel) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            message_id: value.message_id,
            peer_id: value.peer_id,
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
    pub peer_id: String,
    pub last_message_id: i64,
    pub last_message_body: String,
    pub last_message_direction: MessageDirection,
    pub last_message_status: MessageStatus,
    pub last_message_at: DateTime<Utc>,
}

impl From<&ConversationMessage> for ConversationSummary {
    fn from(value: &ConversationMessage) -> Self {
        Self {
            peer_id: value.peer_id.clone(),
            last_message_id: value.id,
            last_message_body: value.body.clone(),
            last_message_direction: value.direction,
            last_message_status: value.status,
            last_message_at: value.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MessageRecordError {
    #[error("Unknown message direction: {0}")]
    UnknownDirection(String),
    #[error("Unknown message status: {0}")]
    UnknownStatus(String),
}

pub fn build_conversation_caches(
    messages: Vec<ConversationMessage>,
) -> (ConversationMap, Arc<Vec<ConversationSummary>>) {
    let mut grouped = HashMap::<String, Vec<ConversationMessage>>::new();
    for message in messages {
        grouped.entry(message.peer_id.clone()).or_default().push(message);
    }

    let conversations = grouped
        .into_iter()
        .map(|(peer_id, messages)| (peer_id, Arc::new(messages)))
        .collect::<ConversationMap>();
    let summaries = build_conversation_summaries(&conversations);

    (conversations, summaries)
}

pub fn build_conversation_summaries(
    conversations: &ConversationMap,
) -> Arc<Vec<ConversationSummary>> {
    let mut summaries = conversations
        .values()
        .filter_map(|messages| messages.last().map(ConversationSummary::from))
        .collect::<Vec<_>>();

    summaries.sort_by(|left, right| {
        right
            .last_message_at
            .cmp(&left.last_message_at)
            .then_with(|| right.last_message_id.cmp(&left.last_message_id))
    });

    Arc::new(summaries)
}

pub fn upsert_conversation_message(
    conversations: &mut ConversationMap,
    message: ConversationMessage,
) {
    let peer_id = message.peer_id.clone();
    let mut cached_messages =
        conversations.get(&peer_id).map(|messages| (**messages).clone()).unwrap_or_default();

    if let Some(existing) = cached_messages.iter_mut().find(|existing| existing.id == message.id) {
        *existing = message;
    } else {
        cached_messages.push(message);
        cached_messages.sort_by(|left, right| {
            left.created_at.cmp(&right.created_at).then_with(|| left.id.cmp(&right.id))
        });
    }

    conversations.insert(peer_id, Arc::new(cached_messages));
}

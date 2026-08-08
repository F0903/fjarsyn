use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Unvalidated message data exchanged across the persistence port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MessageRecord {
    pub id: i64,
    pub session_id: String,
    pub message_id: String,
    pub peer_id: String,
    pub direction: String,
    pub body: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("message persistence failed: {0}")]
    Storage(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl StoreError {
    pub(crate) fn storage(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(error))
    }
}

#[async_trait]
pub(crate) trait Store: Send + Sync {
    async fn list(&self) -> Result<Vec<MessageRecord>, StoreError>;

    async fn create_outgoing(
        &self,
        session_id: crate::peer_session::SessionId,
        message_id: crate::peer_session::MessageId,
        peer_id: crate::identity::PeerId,
        body: String,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<MessageRecord, StoreError>;

    async fn create_incoming_if_missing(
        &self,
        session_id: crate::peer_session::SessionId,
        message_id: crate::peer_session::MessageId,
        peer_id: crate::identity::PeerId,
        body: String,
        created_at: chrono::DateTime<chrono::Utc>,
        received_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MessageRecord>, StoreError>;

    async fn mark_sent(
        &self,
        session_id: crate::peer_session::SessionId,
        peer_id: crate::identity::PeerId,
        message_id: crate::peer_session::MessageId,
    ) -> Result<Option<MessageRecord>, StoreError>;

    async fn mark_delivered(
        &self,
        session_id: crate::peer_session::SessionId,
        peer_id: crate::identity::PeerId,
        message_id: crate::peer_session::MessageId,
        delivered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MessageRecord>, StoreError>;

    async fn mark_failed(
        &self,
        session_id: crate::peer_session::SessionId,
        peer_id: crate::identity::PeerId,
        message_id: crate::peer_session::MessageId,
    ) -> Result<Option<MessageRecord>, StoreError>;

    async fn mark_unknown(
        &self,
        session_id: crate::peer_session::SessionId,
        peer_id: crate::identity::PeerId,
        message_id: crate::peer_session::MessageId,
    ) -> Result<Option<MessageRecord>, StoreError>;

    async fn mark_all_pending_unknown(&self) -> Result<Vec<MessageRecord>, StoreError>;
}

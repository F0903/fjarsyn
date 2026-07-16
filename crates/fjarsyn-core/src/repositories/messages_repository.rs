use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::{
    Error,
    database::MessageModel,
    peer_session::{MessageId, PeerId, SessionId},
    repositories::MessagesStore,
};

#[derive(Clone, Debug)]
pub struct MessagesRepository {
    db: SqlitePool,
}

impl MessagesRepository {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl MessagesStore for MessagesRepository {
    async fn list(&self) -> Result<Vec<MessageModel>, Error> {
        MessageModel::list(&self.db).await
    }

    async fn create_outgoing(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        peer_id: PeerId,
        body: String,
        created_at: DateTime<Utc>,
    ) -> Result<MessageModel, Error> {
        MessageModel::create_outgoing(
            &self.db,
            &session_id.to_string(),
            &message_id.to_string(),
            peer_id.as_str(),
            &body,
            created_at,
        )
        .await
    }

    async fn create_incoming_if_missing(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        peer_id: PeerId,
        body: String,
        created_at: DateTime<Utc>,
        received_at: DateTime<Utc>,
    ) -> Result<Option<MessageModel>, Error> {
        MessageModel::create_incoming_if_missing(
            &self.db,
            &session_id.to_string(),
            &message_id.to_string(),
            peer_id.as_str(),
            &body,
            created_at,
            received_at,
        )
        .await
    }

    async fn mark_sent(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageModel>, Error> {
        MessageModel::mark_sent(
            &self.db,
            &session_id.to_string(),
            peer_id.as_str(),
            &message_id.to_string(),
        )
        .await
    }

    async fn mark_delivered(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        delivered_at: DateTime<Utc>,
    ) -> Result<Option<MessageModel>, Error> {
        MessageModel::mark_delivered(
            &self.db,
            &session_id.to_string(),
            peer_id.as_str(),
            &message_id.to_string(),
            delivered_at,
        )
        .await
    }

    async fn mark_failed(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageModel>, Error> {
        MessageModel::mark_failed(
            &self.db,
            &session_id.to_string(),
            peer_id.as_str(),
            &message_id.to_string(),
        )
        .await
    }

    async fn mark_unknown(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageModel>, Error> {
        MessageModel::mark_unknown(
            &self.db,
            &session_id.to_string(),
            peer_id.as_str(),
            &message_id.to_string(),
        )
        .await
    }

    async fn mark_all_pending_unknown(&self) -> Result<Vec<MessageModel>, Error> {
        MessageModel::mark_all_pending_unknown(&self.db).await
    }
}

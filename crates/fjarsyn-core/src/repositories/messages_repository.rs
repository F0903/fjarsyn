use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::{Error, database::MessageModel, repositories::MessagesStore};

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

    async fn get_by_id(&self, id: i64) -> Result<Option<MessageModel>, Error> {
        MessageModel::get_by_id(&self.db, id).await
    }

    async fn get_by_message_id_and_direction(
        &self,
        message_id: String,
        direction: String,
    ) -> Result<Option<MessageModel>, Error> {
        MessageModel::get_by_message_id_and_direction(&self.db, message_id, direction).await
    }

    async fn create_outgoing(
        &self,
        message_id: String,
        peer_id: String,
        body: String,
        created_at: DateTime<Utc>,
    ) -> Result<i64, Error> {
        MessageModel::create_outgoing(&self.db, message_id, peer_id, body, created_at).await
    }

    async fn create_incoming_if_missing(
        &self,
        message_id: String,
        peer_id: String,
        body: String,
        created_at: DateTime<Utc>,
        delivered_at: DateTime<Utc>,
    ) -> Result<bool, Error> {
        MessageModel::create_incoming_if_missing(
            &self.db,
            message_id,
            peer_id,
            body,
            created_at,
            delivered_at,
        )
        .await
    }

    async fn mark_delivered(
        &self,
        message_id: String,
        delivered_at: DateTime<Utc>,
    ) -> Result<bool, Error> {
        MessageModel::mark_delivered(&self.db, message_id, delivered_at).await
    }

    async fn mark_failed(&self, message_id: String) -> Result<bool, Error> {
        MessageModel::mark_failed(&self.db, message_id).await
    }
}

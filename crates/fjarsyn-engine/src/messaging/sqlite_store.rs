use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::{
    identity::PeerId,
    messaging::{MessageRecord, Store, StoreError},
    peer_session::{MessageId, SessionId},
};

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: i64,
    session_id: String,
    message_id: String,
    peer_id: String,
    direction: String,
    body: String,
    status: String,
    created_at: DateTime<Utc>,
    delivered_at: Option<DateTime<Utc>>,
}

impl From<MessageRow> for MessageRecord {
    fn from(row: MessageRow) -> Self {
        Self {
            id: row.id,
            session_id: row.session_id,
            message_id: row.message_id,
            peer_id: row.peer_id,
            direction: row.direction,
            body: row.body,
            status: row.status,
            created_at: row.created_at,
            delivered_at: row.delivered_at,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SqliteStore {
    db: SqlitePool,
}

impl SqliteStore {
    pub(crate) fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    async fn transition_outgoing(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        status: &str,
        delivered_at: Option<DateTime<Utc>>,
        current_status: &str,
    ) -> Result<Option<MessageRecord>, StoreError> {
        sqlx::query_as::<_, MessageRow>(
            "UPDATE messages
             SET status = ?, delivered_at = ?
             WHERE session_id = ?
               AND peer_id = ?
               AND message_id = ?
               AND direction = 'outgoing'
               AND status = ?
             RETURNING id, session_id, message_id, peer_id, direction, body, status,
                       created_at, delivered_at",
        )
        .bind(status)
        .bind(delivered_at)
        .bind(session_id.to_string())
        .bind(peer_id.as_str())
        .bind(message_id.to_string())
        .bind(current_status)
        .fetch_optional(&self.db)
        .await
        .map(|row| row.map(MessageRecord::from))
        .map_err(StoreError::storage)
    }
}

#[async_trait]
impl Store for SqliteStore {
    async fn list(&self) -> Result<Vec<MessageRecord>, StoreError> {
        sqlx::query_as::<_, MessageRow>(
            "SELECT id, session_id, message_id, peer_id, direction, body, status,
                    created_at, delivered_at
             FROM messages
             ORDER BY created_at ASC, id ASC",
        )
        .fetch_all(&self.db)
        .await
        .map(|rows| rows.into_iter().map(MessageRecord::from).collect())
        .map_err(StoreError::storage)
    }

    async fn create_outgoing(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        peer_id: PeerId,
        body: String,
        created_at: DateTime<Utc>,
    ) -> Result<MessageRecord, StoreError> {
        sqlx::query_as::<_, MessageRow>(
            "INSERT INTO messages (
                session_id, message_id, peer_id, direction, body, status, created_at
             ) VALUES (?, ?, ?, 'outgoing', ?, 'pending', ?)
             RETURNING id, session_id, message_id, peer_id, direction, body, status,
                       created_at, delivered_at",
        )
        .bind(session_id.to_string())
        .bind(message_id.to_string())
        .bind(peer_id.as_str())
        .bind(body)
        .bind(created_at)
        .fetch_one(&self.db)
        .await
        .map(MessageRecord::from)
        .map_err(StoreError::storage)
    }

    async fn create_incoming_if_missing(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        peer_id: PeerId,
        body: String,
        created_at: DateTime<Utc>,
        received_at: DateTime<Utc>,
    ) -> Result<Option<MessageRecord>, StoreError> {
        sqlx::query_as::<_, MessageRow>(
            "INSERT INTO messages (
                session_id, message_id, peer_id, direction, body, status,
                created_at, delivered_at
             ) VALUES (?, ?, ?, 'incoming', ?, 'delivered', ?, ?)
             ON CONFLICT(peer_id, message_id, direction) DO NOTHING
             RETURNING id, session_id, message_id, peer_id, direction, body, status,
                       created_at, delivered_at",
        )
        .bind(session_id.to_string())
        .bind(message_id.to_string())
        .bind(peer_id.as_str())
        .bind(body)
        .bind(created_at)
        .bind(received_at)
        .fetch_optional(&self.db)
        .await
        .map(|row| row.map(MessageRecord::from))
        .map_err(StoreError::storage)
    }

    async fn mark_sent(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageRecord>, StoreError> {
        self.transition_outgoing(session_id, peer_id, message_id, "sent", None, "pending").await
    }

    async fn mark_delivered(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        delivered_at: DateTime<Utc>,
    ) -> Result<Option<MessageRecord>, StoreError> {
        sqlx::query_as::<_, MessageRow>(
            "UPDATE messages
             SET status = 'delivered', delivered_at = ?
             WHERE session_id = ?
               AND peer_id = ?
               AND message_id = ?
               AND direction = 'outgoing'
               AND status IN ('pending', 'sent', 'unknown')
             RETURNING id, session_id, message_id, peer_id, direction, body, status,
                       created_at, delivered_at",
        )
        .bind(delivered_at)
        .bind(session_id.to_string())
        .bind(peer_id.as_str())
        .bind(message_id.to_string())
        .fetch_optional(&self.db)
        .await
        .map(|row| row.map(MessageRecord::from))
        .map_err(StoreError::storage)
    }

    async fn mark_failed(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageRecord>, StoreError> {
        self.transition_outgoing(session_id, peer_id, message_id, "failed", None, "pending").await
    }

    async fn mark_unknown(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageRecord>, StoreError> {
        self.transition_outgoing(session_id, peer_id, message_id, "unknown", None, "pending").await
    }

    async fn mark_all_pending_unknown(&self) -> Result<Vec<MessageRecord>, StoreError> {
        sqlx::query_as::<_, MessageRow>(
            "UPDATE messages
             SET status = 'unknown'
             WHERE direction = 'outgoing' AND status = 'pending'
             RETURNING id, session_id, message_id, peer_id, direction, body, status,
                       created_at, delivered_at",
        )
        .fetch_all(&self.db)
        .await
        .map(|rows| rows.into_iter().map(MessageRecord::from).collect())
        .map_err(StoreError::storage)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sqlx::sqlite::SqliteConnectOptions;

    use super::*;

    async fn test_store() -> (SqliteStore, std::path::PathBuf) {
        let path = std::env::temp_dir()
            .join(format!("fjarsyn-messages-store-{}.db", uuid::Uuid::new_v4()));
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::raw_sql(include_str!("../../migrations/0002_messages.sql"))
            .execute(&pool)
            .await
            .unwrap();
        (SqliteStore::new(pool), path)
    }

    async fn close(store: SqliteStore, path: std::path::PathBuf) {
        store.db.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn receipt_updates_only_the_bound_session_and_peer() {
        let (store, path) = test_store().await;
        let now = Utc::now();
        let session_a = SessionId::new();
        let session_b = SessionId::new();
        let message_id = MessageId::new();
        let peer_a = PeerId::new("peer-a").unwrap();
        let peer_b = PeerId::new("peer-b").unwrap();
        let outgoing = store
            .create_outgoing(session_a, message_id, peer_a.clone(), "hello".into(), now)
            .await
            .unwrap();

        assert_eq!(outgoing.status, "pending");
        assert!(store.mark_sent(session_a, peer_a.clone(), message_id).await.unwrap().is_some());
        assert!(store.mark_delivered(session_a, peer_b, message_id, now).await.unwrap().is_none());
        assert!(
            store
                .mark_delivered(session_b, peer_a.clone(), message_id, now)
                .await
                .unwrap()
                .is_none()
        );

        let delivered =
            store.mark_delivered(session_a, peer_a, message_id, now).await.unwrap().unwrap();
        assert_eq!(delivered.status, "delivered");
        assert_eq!(delivered.delivered_at, Some(now));

        close(store, path).await;
    }

    #[tokio::test]
    async fn an_uncertain_send_can_still_be_reconciled_by_a_bound_receipt() {
        let (store, path) = test_store().await;
        let now = Utc::now();
        let session_a = SessionId::new();
        let message_a = MessageId::new();
        let peer_a = PeerId::new("peer-a").unwrap();
        store
            .create_outgoing(session_a, message_a, peer_a.clone(), "hello".into(), now)
            .await
            .unwrap();

        let unknown =
            store.mark_unknown(session_a, peer_a.clone(), message_a).await.unwrap().unwrap();
        assert_eq!(unknown.status, "unknown");

        let delivered =
            store.mark_delivered(session_a, peer_a, message_a, now).await.unwrap().unwrap();
        assert_eq!(delivered.status, "delivered");

        store
            .create_outgoing(
                SessionId::new(),
                MessageId::new(),
                PeerId::new("peer-b").unwrap(),
                "interrupted".into(),
                now,
            )
            .await
            .unwrap();
        let recovered = store.mark_all_pending_unknown().await.unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].status, "unknown");

        close(store, path).await;
    }

    #[tokio::test]
    async fn incoming_duplicates_are_scoped_to_the_authenticated_peer() {
        let (store, path) = test_store().await;
        let now = Utc::now();
        let message_id = MessageId::new();
        let peer_a = PeerId::new("peer-a").unwrap();

        assert!(
            store
                .create_incoming_if_missing(
                    SessionId::new(),
                    message_id,
                    peer_a.clone(),
                    "hello".into(),
                    now,
                    now,
                )
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .create_incoming_if_missing(
                    SessionId::new(),
                    message_id,
                    peer_a,
                    "replayed".into(),
                    now,
                    now,
                )
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .create_incoming_if_missing(
                    SessionId::new(),
                    message_id,
                    PeerId::new("peer-b").unwrap(),
                    "independent".into(),
                    now,
                    now,
                )
                .await
                .unwrap()
                .is_some()
        );

        let messages = store.list().await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].body, "hello");

        close(store, path).await;
    }
}

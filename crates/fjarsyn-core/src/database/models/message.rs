use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct MessageModel {
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

impl MessageModel {
    pub async fn list(pool: &sqlx::SqlitePool) -> Result<Vec<Self>, crate::Error> {
        sqlx::query_as::<_, Self>(
            "SELECT id, session_id, message_id, peer_id, direction, body, status,
                    created_at, delivered_at
             FROM messages
             ORDER BY created_at ASC, id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(crate::Error::DatabaseError)
    }

    pub async fn create_outgoing(
        pool: &sqlx::SqlitePool,
        session_id: &str,
        message_id: &str,
        peer_id: &str,
        body: &str,
        created_at: DateTime<Utc>,
    ) -> Result<Self, crate::Error> {
        sqlx::query_as::<_, Self>(
            "INSERT INTO messages (
                session_id, message_id, peer_id, direction, body, status, created_at
             ) VALUES (?, ?, ?, 'outgoing', ?, 'pending', ?)
             RETURNING id, session_id, message_id, peer_id, direction, body, status,
                       created_at, delivered_at",
        )
        .bind(session_id)
        .bind(message_id)
        .bind(peer_id)
        .bind(body)
        .bind(created_at)
        .fetch_one(pool)
        .await
        .map_err(crate::Error::DatabaseError)
    }

    pub async fn create_incoming_if_missing(
        pool: &sqlx::SqlitePool,
        session_id: &str,
        message_id: &str,
        peer_id: &str,
        body: &str,
        created_at: DateTime<Utc>,
        received_at: DateTime<Utc>,
    ) -> Result<Option<Self>, crate::Error> {
        sqlx::query_as::<_, Self>(
            "INSERT INTO messages (
                session_id, message_id, peer_id, direction, body, status,
                created_at, delivered_at
             ) VALUES (?, ?, ?, 'incoming', ?, 'delivered', ?, ?)
             ON CONFLICT(peer_id, message_id, direction) DO NOTHING
             RETURNING id, session_id, message_id, peer_id, direction, body, status,
                       created_at, delivered_at",
        )
        .bind(session_id)
        .bind(message_id)
        .bind(peer_id)
        .bind(body)
        .bind(created_at)
        .bind(received_at)
        .fetch_optional(pool)
        .await
        .map_err(crate::Error::DatabaseError)
    }

    pub async fn mark_sent(
        pool: &sqlx::SqlitePool,
        session_id: &str,
        peer_id: &str,
        message_id: &str,
    ) -> Result<Option<Self>, crate::Error> {
        Self::transition_outgoing(pool, session_id, peer_id, message_id, "sent", None, "pending")
            .await
    }

    pub async fn mark_delivered(
        pool: &sqlx::SqlitePool,
        session_id: &str,
        peer_id: &str,
        message_id: &str,
        delivered_at: DateTime<Utc>,
    ) -> Result<Option<Self>, crate::Error> {
        sqlx::query_as::<_, Self>(
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
        .bind(session_id)
        .bind(peer_id)
        .bind(message_id)
        .fetch_optional(pool)
        .await
        .map_err(crate::Error::DatabaseError)
    }

    pub async fn mark_failed(
        pool: &sqlx::SqlitePool,
        session_id: &str,
        peer_id: &str,
        message_id: &str,
    ) -> Result<Option<Self>, crate::Error> {
        Self::transition_outgoing(pool, session_id, peer_id, message_id, "failed", None, "pending")
            .await
    }

    pub async fn mark_unknown(
        pool: &sqlx::SqlitePool,
        session_id: &str,
        peer_id: &str,
        message_id: &str,
    ) -> Result<Option<Self>, crate::Error> {
        Self::transition_outgoing(pool, session_id, peer_id, message_id, "unknown", None, "pending")
            .await
    }

    pub async fn mark_all_pending_unknown(
        pool: &sqlx::SqlitePool,
    ) -> Result<Vec<Self>, crate::Error> {
        sqlx::query_as::<_, Self>(
            "UPDATE messages
             SET status = 'unknown'
             WHERE direction = 'outgoing' AND status = 'pending'
             RETURNING id, session_id, message_id, peer_id, direction, body, status,
                       created_at, delivered_at",
        )
        .fetch_all(pool)
        .await
        .map_err(crate::Error::DatabaseError)
    }

    async fn transition_outgoing(
        pool: &sqlx::SqlitePool,
        session_id: &str,
        peer_id: &str,
        message_id: &str,
        status: &str,
        delivered_at: Option<DateTime<Utc>>,
        current_status: &str,
    ) -> Result<Option<Self>, crate::Error> {
        sqlx::query_as::<_, Self>(
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
        .bind(session_id)
        .bind(peer_id)
        .bind(message_id)
        .bind(current_status)
        .fetch_optional(pool)
        .await
        .map_err(crate::Error::DatabaseError)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

    use super::MessageModel;

    async fn test_pool() -> (SqlitePool, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("fjarsyn-message-model-{}.db", uuid::Uuid::new_v4()));
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();

        sqlx::raw_sql(include_str!("../../../migrations/0002_messages.sql"))
            .execute(&pool)
            .await
            .unwrap();

        (pool, path)
    }

    #[tokio::test]
    async fn receipt_updates_only_the_bound_session_and_peer() {
        let (pool, path) = test_pool().await;
        let now = Utc::now();
        let outgoing =
            MessageModel::create_outgoing(&pool, "session-a", "message-a", "peer-a", "hello", now)
                .await
                .unwrap();

        assert_eq!(outgoing.status, "pending");
        assert!(
            MessageModel::mark_sent(&pool, "session-a", "peer-a", "message-a")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            MessageModel::mark_delivered(&pool, "session-a", "peer-b", "message-a", now,)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            MessageModel::mark_delivered(&pool, "session-b", "peer-a", "message-a", now,)
                .await
                .unwrap()
                .is_none()
        );

        let delivered =
            MessageModel::mark_delivered(&pool, "session-a", "peer-a", "message-a", now)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(delivered.status, "delivered");
        assert_eq!(delivered.delivered_at, Some(now));

        pool.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn an_uncertain_send_can_still_be_reconciled_by_a_bound_receipt() {
        let (pool, path) = test_pool().await;
        let now = Utc::now();
        MessageModel::create_outgoing(&pool, "session-a", "message-a", "peer-a", "hello", now)
            .await
            .unwrap();

        let unknown = MessageModel::mark_unknown(&pool, "session-a", "peer-a", "message-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(unknown.status, "unknown");

        let delivered =
            MessageModel::mark_delivered(&pool, "session-a", "peer-a", "message-a", now)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(delivered.status, "delivered");

        MessageModel::create_outgoing(
            &pool,
            "session-b",
            "message-b",
            "peer-b",
            "interrupted",
            now,
        )
        .await
        .unwrap();
        let recovered = MessageModel::mark_all_pending_unknown(&pool).await.unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].status, "unknown");

        pool.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn incoming_duplicates_are_scoped_to_the_authenticated_peer() {
        let (pool, path) = test_pool().await;
        let now = Utc::now();

        assert!(
            MessageModel::create_incoming_if_missing(
                &pool,
                "session-a",
                "same-id",
                "peer-a",
                "hello",
                now,
                now,
            )
            .await
            .unwrap()
            .is_some()
        );
        assert!(
            MessageModel::create_incoming_if_missing(
                &pool,
                "session-b",
                "same-id",
                "peer-a",
                "replayed",
                now,
                now,
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(
            MessageModel::create_incoming_if_missing(
                &pool,
                "session-b",
                "same-id",
                "peer-b",
                "independent",
                now,
                now,
            )
            .await
            .unwrap()
            .is_some()
        );

        let messages = MessageModel::list(&pool).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].body, "hello");

        pool.close().await;
        let _ = fs::remove_file(path);
    }
}

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct MessageModel {
    pub id: i64,
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
            "SELECT id, message_id, peer_id, direction, body, status, created_at, delivered_at
             FROM messages
             ORDER BY created_at ASC, id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(crate::Error::DatabaseError)
    }

    pub async fn create_outgoing(
        pool: &sqlx::SqlitePool,
        message_id: impl Into<String>,
        peer_id: impl Into<String>,
        body: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Result<i64, crate::Error> {
        let result = sqlx::query(
            "INSERT INTO messages (message_id, peer_id, direction, body, status, created_at)
             VALUES (?, ?, 'outgoing', ?, 'pending', ?)",
        )
        .bind(message_id.into())
        .bind(peer_id.into())
        .bind(body.into())
        .bind(created_at)
        .execute(pool)
        .await
        .map_err(crate::Error::DatabaseError)?;

        Ok(result.last_insert_rowid())
    }

    pub async fn create_incoming_if_missing(
        pool: &sqlx::SqlitePool,
        message_id: impl Into<String>,
        peer_id: impl Into<String>,
        body: impl Into<String>,
        created_at: DateTime<Utc>,
        delivered_at: DateTime<Utc>,
    ) -> Result<bool, crate::Error> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO messages (
                message_id,
                peer_id,
                direction,
                body,
                status,
                created_at,
                delivered_at
             ) VALUES (?, ?, 'incoming', ?, 'delivered', ?, ?)",
        )
        .bind(message_id.into())
        .bind(peer_id.into())
        .bind(body.into())
        .bind(created_at)
        .bind(delivered_at)
        .execute(pool)
        .await
        .map_err(crate::Error::DatabaseError)?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_delivered(
        pool: &sqlx::SqlitePool,
        message_id: impl Into<String>,
        delivered_at: DateTime<Utc>,
    ) -> Result<bool, crate::Error> {
        let result = sqlx::query(
            "UPDATE messages
             SET status = 'delivered', delivered_at = ?
             WHERE message_id = ? AND direction = 'outgoing'",
        )
        .bind(delivered_at)
        .bind(message_id.into())
        .execute(pool)
        .await
        .map_err(crate::Error::DatabaseError)?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_failed(
        pool: &sqlx::SqlitePool,
        message_id: impl Into<String>,
    ) -> Result<bool, crate::Error> {
        let result = sqlx::query(
            "UPDATE messages
             SET status = 'failed'
             WHERE message_id = ? AND direction = 'outgoing' AND status = 'pending'",
        )
        .bind(message_id.into())
        .execute(pool)
        .await
        .map_err(crate::Error::DatabaseError)?;

        Ok(result.rows_affected() > 0)
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

        sqlx::query(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id TEXT NOT NULL,
                peer_id TEXT NOT NULL,
                direction TEXT NOT NULL CHECK (direction IN ('incoming', 'outgoing')),
                body TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('pending', 'delivered', 'failed')),
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                delivered_at DATETIME,
                UNIQUE (message_id, direction)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE INDEX idx_messages_peer_created_at
             ON messages (peer_id, created_at, id)",
        )
        .execute(&pool)
        .await
        .unwrap();

        (pool, path)
    }

    #[tokio::test]
    async fn incoming_and_outgoing_can_share_message_id() {
        let (pool, path) = test_pool().await;
        let now = Utc::now();

        MessageModel::create_outgoing(&pool, "same-id", "peer-a", "hello", now).await.unwrap();

        let inserted =
            MessageModel::create_incoming_if_missing(&pool, "same-id", "peer-a", "hello", now, now)
                .await
                .unwrap();

        assert!(inserted);

        let messages = MessageModel::list(&pool).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().any(|message| message.direction == "outgoing"));
        assert!(messages.iter().any(|message| message.direction == "incoming"));

        pool.close().await;
        let _ = fs::remove_file(path);
    }
}

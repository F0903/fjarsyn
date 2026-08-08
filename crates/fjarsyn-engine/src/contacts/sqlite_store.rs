use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use super::{ContactRecord, Store, StoreError};

#[derive(sqlx::FromRow)]
struct ContactRow {
    id: i64,
    peer_id: String,
    name: String,
    trusted_public_key: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ContactRow> for ContactRecord {
    fn from(row: ContactRow) -> Self {
        Self {
            id: row.id,
            peer_id: row.peer_id,
            name: row.name,
            trusted_public_key: row.trusted_public_key,
            created_at: row.created_at,
            updated_at: row.updated_at,
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
}

#[async_trait]
impl Store for SqliteStore {
    async fn list(&self) -> Result<Vec<ContactRecord>, StoreError> {
        sqlx::query_as::<_, ContactRow>(
            "SELECT id, peer_id, name, trusted_public_key, created_at, updated_at
             FROM contacts
             ORDER BY id DESC",
        )
        .fetch_all(&self.db)
        .await
        .map(|rows| rows.into_iter().map(ContactRecord::from).collect())
        .map_err(StoreError::storage)
    }

    async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError> {
        sqlx::query_as::<_, ContactRow>(
            "INSERT INTO contacts (peer_id, name, trusted_public_key)
             VALUES (?, ?, ?)
             RETURNING id, peer_id, name, trusted_public_key, created_at, updated_at",
        )
        .bind(peer_id)
        .bind(name)
        .bind(trusted_public_key)
        .fetch_one(&self.db)
        .await
        .map(ContactRecord::from)
        .map_err(StoreError::storage)
    }

    async fn delete(&self, id: i64) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM contacts WHERE id = ?")
            .bind(id)
            .execute(&self.db)
            .await
            .map_err(StoreError::storage)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound { id });
        }
        Ok(())
    }

    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError> {
        let updated = sqlx::query_as::<_, ContactRow>(
            "UPDATE contacts
             SET peer_id = ?, name = ?, trusted_public_key = ?, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?
             RETURNING id, peer_id, name, trusted_public_key, created_at, updated_at",
        )
        .bind(peer_id)
        .bind(name)
        .bind(trusted_public_key)
        .bind(id)
        .fetch_optional(&self.db)
        .await
        .map_err(StoreError::storage)?;
        updated.map(ContactRecord::from).ok_or(StoreError::NotFound { id })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sqlx::sqlite::SqliteConnectOptions;

    use super::*;

    async fn test_store() -> (SqliteStore, std::path::PathBuf) {
        let path = std::env::temp_dir()
            .join(format!("fjarsyn-contacts-store-{}.db", uuid::Uuid::new_v4()));
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        (SqliteStore::new(pool), path)
    }

    async fn get_by_id(store: &SqliteStore, id: i64) -> Option<ContactRecord> {
        sqlx::query_as::<_, ContactRow>(
            "SELECT id, peer_id, name, trusted_public_key, created_at, updated_at
             FROM contacts
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&store.db)
        .await
        .unwrap()
        .map(ContactRecord::from)
    }

    async fn close(store: SqliteStore, path: std::path::PathBuf) {
        store.db.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn contact_identity_fields_are_required_and_unique() {
        let (store, path) = test_store().await;

        store.create("peer-a".into(), "Alice".into(), "key-a".into()).await.unwrap();

        assert!(store.create("peer-a".into(), "Other Alice".into(), "key-b".into()).await.is_err());
        assert!(store.create("peer-b".into(), "Bob".into(), "key-a".into()).await.is_err());

        close(store, path).await;
    }

    #[tokio::test]
    async fn update_preserves_creation_time_and_refreshes_update_time() {
        let (store, path) = test_store().await;
        let id = store.create("peer-a".into(), "Alice".into(), "key-a".into()).await.unwrap().id;

        sqlx::query("UPDATE contacts SET updated_at = '2000-01-01 00:00:00' WHERE id = ?")
            .bind(id)
            .execute(&store.db)
            .await
            .unwrap();
        let before = get_by_id(&store, id).await.unwrap();

        store
            .update(id, "peer-a".into(), "Alice Updated".into(), "key-a-updated".into())
            .await
            .unwrap();
        let after = get_by_id(&store, id).await.unwrap();

        assert_eq!(after.name, "Alice Updated");
        assert_eq!(after.trusted_public_key, "key-a-updated");
        assert_eq!(after.created_at, before.created_at);
        assert!(after.updated_at > before.updated_at);

        close(store, path).await;
    }

    #[tokio::test]
    async fn update_and_delete_reject_unknown_ids() {
        let (store, path) = test_store().await;

        assert!(matches!(
            store.update(404, "peer-a".into(), "Alice".into(), "key-a".into()).await,
            Err(StoreError::NotFound { id: 404 })
        ));
        assert!(matches!(store.delete(404).await, Err(StoreError::NotFound { id: 404 })));

        close(store, path).await;
    }
}

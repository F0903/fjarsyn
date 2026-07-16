use crate::define_model;

define_model!(
    ContactModel,
    "contacts",
    fields: {
        peer_id: String,
        name: String,
        trusted_public_key: String,
    },
    create: {
        sql: "INSERT INTO contacts (peer_id, name, trusted_public_key) VALUES (?, ?, ?) RETURNING *",
        params: [peer_id, name, trusted_public_key]
    },
    update: {
        sql: "UPDATE contacts SET peer_id = ?, name = ?, trusted_public_key = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ? RETURNING *",
        params: [peer_id, name, trusted_public_key]
    }
);

#[cfg(test)]
mod tests {
    use std::fs;

    use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

    use super::ContactModel;

    async fn test_pool() -> (SqlitePool, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("fjarsyn-contact-model-{}.db", uuid::Uuid::new_v4()));
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        (pool, path)
    }

    #[tokio::test]
    async fn contact_identity_fields_are_required_and_unique() {
        let (pool, path) = test_pool().await;

        ContactModel::create(&pool, "peer-a", "Alice", "key-a").await.unwrap();

        assert!(ContactModel::create(&pool, "peer-a", "Other Alice", "key-b").await.is_err());
        assert!(ContactModel::create(&pool, "peer-b", "Bob", "key-a").await.is_err());

        pool.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn update_preserves_creation_time_and_refreshes_update_time() {
        let (pool, path) = test_pool().await;
        let id = ContactModel::create(&pool, "peer-a", "Alice", "key-a").await.unwrap().id;

        sqlx::query("UPDATE contacts SET updated_at = '2000-01-01 00:00:00' WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let before = ContactModel::get_by_id(&pool, id).await.unwrap().unwrap();

        ContactModel::update(&pool, id, "peer-a", "Alice Updated", "key-a-updated").await.unwrap();
        let after = ContactModel::get_by_id(&pool, id).await.unwrap().unwrap();

        assert_eq!(after.name, "Alice Updated");
        assert_eq!(after.trusted_public_key, "key-a-updated");
        assert_eq!(after.created_at, before.created_at);
        assert!(after.updated_at > before.updated_at);

        pool.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn update_and_delete_reject_unknown_ids() {
        let (pool, path) = test_pool().await;

        assert!(matches!(
            ContactModel::update(&pool, 404, "peer-a", "Alice", "key-a").await,
            Err(crate::Error::RecordNotFound { id: 404, .. })
        ));
        assert!(matches!(
            ContactModel::delete(&pool, 404).await,
            Err(crate::Error::RecordNotFound { id: 404, .. })
        ));

        pool.close().await;
        let _ = fs::remove_file(path);
    }
}

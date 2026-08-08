use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Unvalidated contact data exchanged across the persistence port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContactRecord {
    pub id: i64,
    pub peer_id: String,
    pub name: String,
    pub trusted_public_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("contact record {id} was not found")]
    NotFound { id: i64 },
    #[error("contact persistence failed: {0}")]
    Storage(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl StoreError {
    pub(crate) fn storage(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Storage(Box::new(error))
    }
}

#[async_trait]
pub(crate) trait Store: Send + Sync {
    async fn list(&self) -> Result<Vec<ContactRecord>, StoreError>;

    async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError>;

    async fn delete(&self, id: i64) -> Result<(), StoreError>;

    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError>;
}

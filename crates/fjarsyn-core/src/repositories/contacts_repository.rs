use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::{Error, database::ContactModel, repositories::ContactsStore};

#[derive(Clone, Debug)]
pub struct ContactsRepository {
    db: SqlitePool,
}

impl ContactsRepository {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ContactsStore for ContactsRepository {
    async fn list(&self) -> Result<Vec<ContactModel>, Error> {
        ContactModel::list(&self.db).await
    }

    async fn get_by_id(&self, id: i64) -> Result<Option<ContactModel>, Error> {
        ContactModel::get_by_id(&self.db, id).await
    }

    async fn create(
        &self,
        peer_id: String,
        name: String,
        address: Option<String>,
        trusted_public_key: Option<String>,
    ) -> Result<i64, Error> {
        ContactModel::create(&self.db, peer_id, name, address, trusted_public_key).await
    }

    async fn delete(&self, id: i64) -> Result<(), Error> {
        ContactModel::delete(&self.db, id).await
    }

    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        address: Option<String>,
        trusted_public_key: Option<String>,
    ) -> Result<(), Error> {
        ContactModel::update(&self.db, id, peer_id, name, address, trusted_public_key).await
    }
}

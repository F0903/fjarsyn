use std::sync::Arc;

use sqlx::SqlitePool;

use crate::{Error, database::Contact};

#[derive(Clone)]
pub struct ContactsService {
    db: SqlitePool,
}

impl ContactsService {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<Contact>, Arc<Error>> {
        Contact::list(&self.db).await.map_err(Arc::new)
    }

    pub async fn create(
        &self,
        peer_id: String,
        name: String,
        address: Option<String>,
    ) -> Result<i64, Arc<Error>> {
        Contact::create(&self.db, peer_id, name, address).await.map_err(Arc::new)
    }

    pub async fn delete(&self, id: i64) -> Result<(), Arc<Error>> {
        Contact::delete(&self.db, id).await.map_err(Arc::new)
    }

    pub async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        address: Option<String>,
    ) -> Result<(), Arc<Error>> {
        Contact::update(&self.db, id, peer_id, name, address).await.map_err(Arc::new)
    }
}

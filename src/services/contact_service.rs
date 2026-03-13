use std::sync::Arc;

use sqlx::SqlitePool;

use crate::{Error, database::Contact};

pub struct ContactService;

impl ContactService {
    pub async fn list(db: &SqlitePool) -> Result<Vec<Contact>, Arc<Error>> {
        Contact::list(db).await.map_err(Arc::new)
    }

    pub async fn create(
        db: &SqlitePool,
        peer_id: String,
        name: String,
        address: Option<String>,
    ) -> Result<i64, Arc<Error>> {
        Contact::create(db, peer_id, name, address).await.map_err(Arc::new)
    }

    pub async fn delete(db: &SqlitePool, id: i64) -> Result<(), Arc<Error>> {
        Contact::delete(db, id).await.map_err(Arc::new)
    }

    pub async fn update(
        db: &SqlitePool,
        id: i64,
        peer_id: String,
        name: String,
        address: Option<String>,
    ) -> Result<(), Arc<Error>> {
        Contact::update(db, id, peer_id, name, address).await.map_err(Arc::new)
    }
}

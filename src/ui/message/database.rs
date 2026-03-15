use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum DatabaseMessage {
    DatabaseInitialized(Result<sqlx::SqlitePool, Arc<crate::Error>>),
}

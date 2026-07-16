use std::{fs, path::PathBuf, sync::LazyLock, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

pub mod macros;
pub mod models;

pub use models::*;

use crate::utils::paths::DATA_DIR;

// This architecture deliberately starts with a clean schema. Keeping a distinct
// canonical file also leaves pre-rework development data untouched instead of
// attempting to interpret migrations whose history was intentionally replaced.
static DB_PATH: LazyLock<PathBuf> = LazyLock::new(|| DATA_DIR.join("fjarsyn-peer-sessions.db"));

pub async fn init() -> Result<SqlitePool, crate::Error> {
    if let Some(parent) = DB_PATH.parent() {
        fs::create_dir_all(parent)?;
    }

    let database_timeout = Duration::from_secs(5);
    let options = SqliteConnectOptions::new()
        .filename(&*DB_PATH)
        .create_if_missing(true)
        .busy_timeout(database_timeout);
    let pool = SqlitePoolOptions::new()
        .acquire_timeout(database_timeout)
        .connect_with(options)
        .await
        .map_err(crate::Error::DatabaseError)?;

    sqlx::migrate!("./migrations").run(&pool).await.map_err(crate::Error::MigrationError)?;

    Ok(pool)
}

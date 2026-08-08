//! SQLite initialization and infrastructure errors.

use std::{fs, path::PathBuf, sync::LazyLock, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use crate::paths::DATA_DIR;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("failed to create the database directory: {0}")]
    Directory(#[from] std::io::Error),
    #[error("failed to connect to the database: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("failed to migrate the database: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

// This architecture deliberately starts with a clean schema. Keeping a distinct
// canonical file also leaves pre-rework development data untouched instead of
// attempting to interpret migrations whose history was intentionally replaced.
static DB_PATH: LazyLock<PathBuf> = LazyLock::new(|| DATA_DIR.join("fjarsyn-peer-sessions.db"));

pub(crate) async fn init() -> Result<SqlitePool, Error> {
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
        .map_err(Error::Connect)?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

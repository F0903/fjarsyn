use std::{fs, path::PathBuf, sync::LazyLock};

use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

pub mod macros;
pub mod models;

pub use models::*;

use crate::utils::paths::DATA_DIR;

static DB_PATH: LazyLock<PathBuf> = LazyLock::new(|| DATA_DIR.join("fjarsyn.db"));

pub async fn init() -> Result<SqlitePool, crate::Error> {
    if let Some(parent) = DB_PATH.parent() {
        fs::create_dir_all(parent)?;
    }

    let options = SqliteConnectOptions::new().filename(&*DB_PATH).create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await.map_err(crate::Error::DatabaseError)?;

    sqlx::migrate!("./migrations").run(&pool).await.map_err(crate::Error::MigrationError)?;

    Ok(pool)
}

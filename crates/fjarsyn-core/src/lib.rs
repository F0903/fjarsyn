pub mod capture_providers;
pub mod communication;
pub mod config;
pub mod database;
pub mod identity;
pub mod media;
pub mod pairing;
pub mod peer_session;
pub mod presence;
pub mod repositories;
pub mod services;
pub mod utils;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Migration error: {0}")]
    MigrationError(#[from] sqlx::migrate::MigrateError),
    #[error("{entity} record {id} was not found")]
    RecordNotFound { entity: &'static str, id: i64 },
    #[error("invalid contact: {0}")]
    InvalidContact(String),
    #[error("Capture error: {0}")]
    CaptureError(#[from] capture_providers::CaptureError),
    #[error("Windows capture builder error: {0}")]
    WindowsCaptureBuilderError(#[from] capture_providers::windows::WgcCaptureProviderBuilderError),
    #[error("Windows error: {0}")]
    WindowsError(#[from] windows_core::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

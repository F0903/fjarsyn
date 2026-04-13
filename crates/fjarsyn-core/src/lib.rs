pub mod app;
pub mod capture_providers;
pub mod communication;
pub mod config;
pub mod database;
pub mod executors;
pub mod media;
pub mod networking;
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
    #[error("Capture error: {0}")]
    CaptureError(#[from] capture_providers::CaptureError),
    #[error("Windows capture builder error: {0}")]
    WindowsCaptureBuilderError(#[from] capture_providers::windows::WgcCaptureProviderBuilderError),
    #[error("Windows error: {0}")]
    WindowsError(#[from] windows_core::Error),
    #[error("WebRTC error: {0}")]
    WebRtcError(#[from] networking::webrtc::WebRTCError),
}

pub type Result<T> = std::result::Result<T, Error>;

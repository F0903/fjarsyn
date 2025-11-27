use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use crate::capture_providers::CaptureError;

mod capture_providers;
mod ui;
mod utils;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Capture error: {0}")]
    CaptureError(#[from] capture_providers::CaptureError),
    #[error("Windows capture builder error: {0}")]
    WindowsCaptureBuilderError(#[from] capture_providers::windows::BuilderError),
    #[error("Windows capture error: {0}")]
    WindowsError(#[from] windows_core::Error),
    #[error("UI error: {0}")]
    UiError(#[from] iced::Error),
    #[error("UI window management error: {0}")]
    UiWindowMgmtError(#[from] iced_winit::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Other error: {0}")]
    OtherError(#[from] Box<dyn std::error::Error>),
}

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::TRACE).finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    tracing::info!("Starting up...");

    tracing::info!("Initializing windows capture provider...");
    let windows_capture = capture_providers::windows::WindowsCaptureProviderBuilder::new()
        .with_default_device()?
        .with_default_capture_item()?
        .build()?;
    let windows_capture = Arc::new(Mutex::new(windows_capture));
    tracing::info!("Windows capture provider initialized.");

    tracing::info!("Initializing UI...");
    let app = ui::app::App::new(windows_capture)?;
    tracing::info!("UI initialized.");

    tracing::info!("Running app...");
    app.run()?;
    tracing::info!("App exited.");

    Ok(())
}

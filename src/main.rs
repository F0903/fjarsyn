use std::sync::Arc;

use loki::{Result, capture_providers, ui};
use tokio::sync::Mutex;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::TRACE).finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    tracing::info!("Starting up...");

    tracing::info!("Initializing windows capture provider...");
    let windows_capture = capture_providers::windows::WgcCaptureProviderBuilder::new()
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

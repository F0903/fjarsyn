#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::sync::Arc;

use fjarsyn::{Result, capture_providers, ui};
use tokio::sync::RwLock;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[cfg(debug_assertions)]
const LOG_LEVEL: Level = Level::TRACE;
#[cfg(not(debug_assertions))]
const LOG_LEVEL: Level = Level::INFO;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder().with_max_level(LOG_LEVEL).finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    tracing::info!("Starting up...");

    let initial_config = fjarsyn::config::Config::load();

    tracing::info!("Initializing windows capture provider...");
    let windows_capture = capture_providers::windows::WgcCaptureProviderBuilder::new(
        initial_config.pixel_format,
        initial_config.record_cursor,
        initial_config.recording_border_indicator,
    )
    .with_default_device()?
    .with_default_capture_item()?
    .build()?;
    let windows_capture = Arc::new(RwLock::new(windows_capture));
    tracing::info!("Windows capture provider initialized.");

    tracing::info!("Running app...");
    iced::daemon(move || ui::app::init(windows_capture.clone()), ui::app::update, ui::app::view)
        .subscription(ui::app::subscription)
        .title(ui::app::title)
        .run()?;
    tracing::info!("App exited.");

    Ok(())
}

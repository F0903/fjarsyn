//! Fjarsyn desktop application composition, runtime ownership, and user interface.

#![deny(unreachable_pub)]

mod error;
mod settings;
mod ui;
mod wgpu;

pub use error::{Error, Result};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[cfg(debug_assertions)]
const LOG_LEVEL: Level = Level::TRACE;
#[cfg(not(debug_assertions))]
const LOG_LEVEL: Level = Level::INFO;

/// Starts the Fjarsyn desktop application and runs it until its UI exits.
pub fn run() -> Result<()> {
    let settings_store = settings::Store::system()?;
    let settings = settings_store.load_or_create()?;

    wgpu::apply_wgpu_power_preference(settings.power_preference);
    wgpu::configure_default_wgpu_backend();

    tracing::subscriber::set_global_default(
        FmtSubscriber::builder().with_max_level(LOG_LEVEL).finish(),
    )
    .expect("setting default subscriber failed");

    tracing::info!("Starting app...");
    ui::run(settings, settings_store)?;
    tracing::info!("App exited.");

    Ok(())
}

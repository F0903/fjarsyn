//! Fjarsyn desktop application composition, runtime ownership, and user interface.

#![deny(unreachable_pub)]

mod error;
mod ui;
mod wgpu;

pub use error::{Error, Result};
use fjarsyn_engine::config::Config;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[cfg(debug_assertions)]
const LOG_LEVEL: Level = Level::TRACE;
#[cfg(not(debug_assertions))]
const LOG_LEVEL: Level = Level::INFO;

/// Starts the Fjarsyn desktop application and runs it until its UI exits.
pub fn run() -> Result<()> {
    let config = Config::load_or_create()?;

    wgpu::apply_wgpu_power_pref(config.app.power_pref);
    wgpu::configure_default_wgpu_backend();

    tracing::subscriber::set_global_default(
        FmtSubscriber::builder().with_max_level(LOG_LEVEL).finish(),
    )
    .expect("setting default subscriber failed");

    tracing::info!("Starting app...");
    ui::run(config)?;
    tracing::info!("App exited.");

    Ok(())
}

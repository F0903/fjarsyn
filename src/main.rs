#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use fjarsyn::{Result, ui::app::Fjarsyn};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[cfg(debug_assertions)]
const LOG_LEVEL: Level = Level::TRACE;
#[cfg(not(debug_assertions))]
const LOG_LEVEL: Level = Level::INFO;

fn main() -> Result<()> {
    fjarsyn::media::gpu_interop::configure_default_wgpu_backend();

    tracing::subscriber::set_global_default(
        FmtSubscriber::builder().with_max_level(LOG_LEVEL).finish(),
    )
    .expect("setting default subscriber failed");

    tracing::info!("Starting app...");
    iced::daemon(Fjarsyn::init, Fjarsyn::update, Fjarsyn::view)
        .subscription(Fjarsyn::subscription)
        .title(Fjarsyn::title)
        .theme(Fjarsyn::theme)
        .default_font(fjarsyn::ui::fonts::outfit::REGULAR)
        .run()?;
    tracing::info!("App exited.");

    Ok(())
}

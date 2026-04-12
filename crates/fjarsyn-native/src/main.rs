#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use fjarsyn_core::media::gpu_interop;
use fjarsyn_native::{Result, ui::app::Fjarsyn};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[cfg(debug_assertions)]
const LOG_LEVEL: Level = Level::TRACE;
#[cfg(not(debug_assertions))]
const LOG_LEVEL: Level = Level::INFO;

fn main() -> Result<()> {
    unsafe {
        std::env::set_var("WGPU_POWER_PREF", "low");
    }

    gpu_interop::configure_default_wgpu_backend();

    tracing::subscriber::set_global_default(
        FmtSubscriber::builder().with_max_level(LOG_LEVEL).finish(),
    )
    .expect("setting default subscriber failed");

    tracing::info!("Starting app...");
    iced::daemon(Fjarsyn::init, Fjarsyn::update, Fjarsyn::view)
        .subscription(Fjarsyn::subscription)
        .title(Fjarsyn::title)
        .theme(Fjarsyn::theme)
        .settings(iced::Settings { antialiasing: false, vsync: false, ..Default::default() })
        .default_font(fjarsyn_native::ui::fonts::outfit::REGULAR)
        .run()?;
    tracing::info!("App exited.");

    Ok(())
}

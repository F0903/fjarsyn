#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use fjarsyn_core::config::Config;
use fjarsyn_native::{Result, ui::shell::Fjarsyn, utils::wgpu};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[cfg(debug_assertions)]
const LOG_LEVEL: Level = Level::TRACE;
#[cfg(not(debug_assertions))]
const LOG_LEVEL: Level = Level::INFO;

fn main() -> Result<()> {
    let config = Config::load_or_create()?;

    wgpu::apply_wgpu_power_pref(config.app.power_pref);
    wgpu::configure_default_wgpu_backend();

    tracing::subscriber::set_global_default(
        FmtSubscriber::builder().with_max_level(LOG_LEVEL).finish(),
    )
    .expect("setting default subscriber failed");

    tracing::info!("Starting app...");
    iced::daemon(move || Fjarsyn::init(config.clone()), Fjarsyn::update, Fjarsyn::view)
        .subscription(Fjarsyn::subscription)
        .title(Fjarsyn::title)
        .theme(Fjarsyn::theme)
        .settings(iced::Settings { antialiasing: false, vsync: false, ..Default::default() })
        .default_font(fjarsyn_native::ui::fonts::outfit::REGULAR)
        .run()?;
    tracing::info!("App exited.");

    Ok(())
}

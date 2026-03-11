#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use fjarsyn::{Result, ui};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[cfg(debug_assertions)]
const LOG_LEVEL: Level = Level::TRACE;
#[cfg(not(debug_assertions))]
const LOG_LEVEL: Level = Level::INFO;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder().with_max_level(LOG_LEVEL).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    tracing::info!("Starting app...");

    iced::daemon(ui::handlers::init, ui::handlers::update, ui::layout::view)
        .subscription(ui::subscription::subscription)
        .title(ui::layout::title)
        .theme(ui::theme::theme_fn)
        .default_font(ui::fonts::outfit::REGULAR)
        .run()?;

    tracing::info!("App exited.");
    Ok(())
}

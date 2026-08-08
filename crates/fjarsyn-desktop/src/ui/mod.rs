//! Desktop interface, presentation, runtime, and shell composition.

mod components;
mod fonts;
mod message;
mod notification;
mod presentation;
mod runtime;
mod screens;
mod shell;
mod subscription;
mod theme;

use fjarsyn_engine::config::Config;
use shell::Fjarsyn;

pub(in crate::ui) const APP_TITLE: &str = "Fjarsyn";

pub(super) fn run(config: Config) -> Result<(), iced::Error> {
    iced::daemon(move || Fjarsyn::init(config.clone()), Fjarsyn::update, Fjarsyn::view)
        .subscription(Fjarsyn::subscription)
        .title(Fjarsyn::title)
        .theme(Fjarsyn::theme)
        .settings(iced::Settings { antialiasing: false, vsync: false, ..Default::default() })
        .default_font(fonts::REGULAR)
        .run()
}

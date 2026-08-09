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

use shell::Fjarsyn;

use crate::settings::{Settings, Store};

pub(in crate::ui) const APP_TITLE: &str = "Fjarsyn";

pub(super) fn run(settings: Settings, settings_store: Store) -> Result<(), iced::Error> {
    iced::daemon(
        move || Fjarsyn::init(settings.clone(), settings_store.clone()),
        Fjarsyn::update,
        Fjarsyn::view,
    )
    .subscription(Fjarsyn::subscription)
    .title(Fjarsyn::title)
    .theme(Fjarsyn::theme)
    .settings(iced::Settings { antialiasing: false, vsync: false, ..Default::default() })
    .default_font(fonts::REGULAR)
    .run()
}

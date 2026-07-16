use std::sync::Arc;

use fjarsyn_core::config::Config;
use iced::{Task, window as iced_window};

use super::Fjarsyn;
use crate::ui::{
    message::{Message, RuntimeMessage},
    runtime::{RuntimeSlot, start_application_runtime},
};

impl Fjarsyn {
    pub fn init(config: Config) -> (Self, Task<Message>) {
        let app = Self::new(config.clone());
        let runtime = Self::start_runtime_task(config, app.runtime.event_tx.clone());
        (app, Task::batch([runtime, Self::open_window_task(), Self::load_fonts_task()]))
    }

    pub(crate) fn start_runtime_task(
        config: Config,
        event_tx: tokio::sync::mpsc::Sender<crate::ui::runtime::RuntimeEvent>,
    ) -> Task<Message> {
        Task::future(async move {
            let result = start_application_runtime(config, event_tx)
                .await
                .map(RuntimeSlot::new)
                .map_err(Arc::new);
            Message::Runtime(RuntimeMessage::Initialized(result))
        })
    }

    fn open_window_task() -> Task<Message> {
        use crate::ui::message::WindowEventMessage;
        iced_window::open(iced_window::Settings {
            decorations: false,
            min_size: Some(iced::Size::new(800.0, 600.0)),
            #[cfg(target_os = "windows")]
            platform_specific: iced_window::settings::PlatformSpecific {
                undecorated_shadow: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .1
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowOpened(id)))
    }

    fn load_fonts_task() -> Task<Message> {
        use crate::ui::fonts::{geist, outfit};
        Task::batch([
            iced::font::load(iced_fonts::LUCIDE_FONT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::THIN_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::EXTRA_LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::REGULAR_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::MEDIUM_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::SEMIBOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::EXTRA_BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::BLACK_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::THIN_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::EXTRA_LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::REGULAR_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::MEDIUM_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::SEMIBOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::EXTRA_BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::BLACK_BYTES).map(|_| Message::NoOp),
        ])
    }
}

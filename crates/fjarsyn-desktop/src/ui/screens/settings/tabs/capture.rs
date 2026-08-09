use iced::{
    Element,
    widget::{checkbox, column, scrollable},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message},
    screens::settings::{
        SettingsDraft,
        components::{setting_row, settings_section},
        tabs::Tab,
    },
};

#[derive(Debug)]
pub(super) struct Capture;

impl Tab for Capture {
    fn id(&self) -> message::screen::settings::TabId {
        message::screen::settings::TabId::Capture
    }

    fn label(&self) -> &'static str {
        "Capture"
    }

    fn icon(&self) -> iced::widget::Text<'static> {
        lucide::clapperboard()
    }

    fn view<'a>(&self, draft: &'a SettingsDraft) -> Element<'a, Message> {
        let content = column![settings_section(
            lucide::clapperboard(),
            "Capture",
            column![
                setting_row(
                    "Record Cursor",
                    "Show the mouse cursor in the recording",
                    checkbox(draft.capture.record_cursor).on_toggle(|val| {
                        Message::Screen(message::Screen::Settings(
                            message::screen::settings::Message::RecordCursorChanged(val),
                        ))
                    }),
                ),
                setting_row(
                    "Border Indicator",
                    "Show a yellow border around the recorded area",
                    checkbox(draft.capture.recording_border_indicator).on_toggle(|val| {
                        Message::Screen(message::Screen::Settings(
                            message::screen::settings::Message::RecordingBorderIndicatorChanged(
                                val,
                            ),
                        ))
                    }),
                ),
                setting_row(
                    "UI Preview",
                    "Show a local preview of the captured area (may use CPU readback when needed)",
                    checkbox(draft.capture.enable_ui_preview).on_toggle(|val| {
                        Message::Screen(message::Screen::Settings(
                            message::screen::settings::Message::EnableUiPreviewChanged(val),
                        ))
                    }),
                ),
            ]
            .spacing(20),
        ),]
        .spacing(25);

        scrollable(content).into()
    }
}

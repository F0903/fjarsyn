use iced::{
    Element,
    widget::{checkbox, column, scrollable},
};
use iced_fonts::lucide;

use crate::{
    config::Config,
    define_tab,
    ui::{
        message::{Message, ScreenMessage},
        screens::settings::{
            SettingsMessage,
            components::{setting_row, settings_section},
            tabs::SettingsTab,
        },
    },
};

define_tab! {
    Capture,
    icon: lucide::clapperboard(),
    view: |config| {
        let content = column![
            settings_section(
                lucide::clapperboard(),
                "Capture",
                column![
                    setting_row(
                        "Record Cursor",
                        "Show the mouse cursor in the recording",
                        checkbox(config.record_cursor).on_toggle(|val| {
                            Message::Screen(ScreenMessage::Settings(
                                SettingsMessage::RecordCursorChanged(val),
                            ))
                        }),
                    ),
                     setting_row(
                        "Border Indicator",
                        "Show a yellow border around the recorded area",
                        checkbox(config.recording_border_indicator).on_toggle(|val| {
                            Message::Screen(ScreenMessage::Settings(
                                SettingsMessage::RecordingBorderIndicatorChanged(val),
                            ))
                        }),
                    ),
                    setting_row(
                        "UI Preview",
                        "Show a local preview of the captured area (may use CPU readback when needed)",
                        checkbox(config.enable_ui_preview).on_toggle(|val| {
                            Message::Screen(ScreenMessage::Settings(
                                SettingsMessage::EnableUiPreviewChanged(val),
                            ))
                        }),
                    ),
                ]
                .spacing(20),
            ),
        ]
        .spacing(25);

        scrollable(content).into()
    }
}

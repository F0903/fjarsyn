use iced::{
    Element,
    widget::{checkbox, column, scrollable},
};
use iced_fonts::lucide;

use crate::{
    config::Config,
    define_tab,
    ui::{
        message::Message,
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
                            Message::Settings(SettingsMessage::RecordCursorChanged(val))
                        }),
                    ),
                     setting_row(
                        "Border Indicator",
                        "Show a yellow border around the recorded area",
                        checkbox(config.recording_border_indicator).on_toggle(|val| {
                            Message::Settings(SettingsMessage::RecordingBorderIndicatorChanged(val))
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

use iced::{
    Alignment, Element, Length,
    widget::{column, row, scrollable, slider, text, text_input},
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
        theme,
    },
};

define_tab! {
    Network,
    icon: lucide::network(),
    view: |config| {
        let content = column![
            settings_section(
                lucide::network(),
                "Network",
                column![
                    setting_row(
                        "Jitter Buffer",
                        "Maximum latency for the depacketizer",
                        row![
                            slider(0..=5000, config.max_depacket_latency, |val| {
                                Message::Settings(SettingsMessage::MaxDepacketLatencyChanged(val))
                            })
                            .width(Length::Fill),
                            text_input("Latency", &format!("{}", config.max_depacket_latency))
                                .on_input(|val| Message::Settings(
                                    SettingsMessage::MaxDepacketLatencyInputChanged(val)
                                ))
                                .padding(8)
                                .width(Length::Fixed(60.0))
                                .style(theme::text_input_style),
                            text("ms").size(12).style(text::secondary)
                        ]
                        .spacing(15)
                        .align_y(Alignment::Center)
                        .width(Length::Fixed(400.0)),
                    ),
                ]
                .spacing(20),
            ),
        ]
        .spacing(25);

        scrollable(content).into()
    }
}

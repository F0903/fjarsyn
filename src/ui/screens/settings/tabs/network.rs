use iced::{
    Alignment, Element, Length,
    widget::{column, row, scrollable, slider, text, text_input},
};
use iced_fonts::lucide;

use crate::{
    config::{Config, NetworkConfig},
    define_tab,
    ui::{
        message::{Message, ScreenMessage},
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
                        "Maximum time to wait for out-of-order video packets",
                        row![
                            slider(
                                0..=NetworkConfig::MAX_DEPACKET_LATENCY_MS,
                                config.network.max_depacket_latency,
                                |val| {
                                Message::Screen(ScreenMessage::Settings(
                                    SettingsMessage::MaxDepacketLatencyChanged(val),
                                ))
                            })
                            .width(Length::Fill),
                            text_input("Latency", &format!("{}", config.network.max_depacket_latency))
                                .on_input(|val| {
                                    Message::Screen(ScreenMessage::Settings(
                                        SettingsMessage::MaxDepacketLatencyInputChanged(val),
                                    ))
                                })
                                .padding(8)
                                .width(Length::Fixed(60.0))
                                .style(theme::text_input_style),
                            text("ms").size(12).style(text::secondary)
                        ]
                        .spacing(15)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                    ),
                ]
                .spacing(20),
            ),
        ]
        .spacing(25);

        scrollable(content).into()
    }
}

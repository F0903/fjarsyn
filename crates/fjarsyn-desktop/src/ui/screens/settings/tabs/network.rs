use fjarsyn_engine::config::{Config, NetworkConfig};
use iced::{
    Alignment, Element, Length,
    widget::{column, row, scrollable, slider, text, text_input},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message},
    screens::settings::{
        components::{setting_row, settings_section},
        tabs::Tab,
    },
    theme,
};

#[derive(Debug)]
pub(super) struct Network;

impl Tab for Network {
    fn id(&self) -> message::screen::settings::TabId {
        message::screen::settings::TabId::Network
    }

    fn label(&self) -> &'static str {
        "Network"
    }

    fn icon(&self) -> iced::widget::Text<'static> {
        lucide::network()
    }

    fn view(&self, config: &Config) -> Element<'_, Message> {
        let content = column![settings_section(
            lucide::network(),
            "Network",
            column![setting_row(
                "Jitter Buffer",
                "Maximum wait for out-of-order video packets (applies after restart)",
                row![
                    slider(
                        0..=NetworkConfig::MAX_DEPACKET_LATENCY_MS,
                        config.network.max_depacket_latency,
                        |val| {
                            Message::Screen(message::Screen::Settings(
                                message::screen::settings::Message::MaxDepacketLatencyChanged(val),
                            ))
                        }
                    )
                    .width(Length::Fill),
                    text_input("Latency", &format!("{}", config.network.max_depacket_latency))
                        .on_input(|val| {
                            Message::Screen(message::Screen::Settings(
                                message::screen::settings::Message::MaxDepacketLatencyInputChanged(
                                    val,
                                ),
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
            ),]
            .spacing(20),
        ),]
        .spacing(25);

        scrollable(content).into()
    }
}

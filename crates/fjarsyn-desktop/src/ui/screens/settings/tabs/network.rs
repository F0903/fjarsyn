use fjarsyn_engine::settings::Network as EngineNetwork;
use iced::{
    Alignment, Element, Length,
    widget::{column, row, scrollable, slider, text, text_input},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{self, Message},
    screens::settings::{
        SettingsDraft,
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

    fn view<'a>(&self, draft: &'a SettingsDraft) -> Element<'a, Message> {
        let latency = row![
            slider(
                0..=EngineNetwork::MAX_DEPACKET_LATENCY_MS,
                draft.network.max_depacket_latency_ms.last_valid(),
                |value| {
                    Message::Screen(message::Screen::Settings(
                        message::screen::settings::Message::MaxDepacketLatencyMsChanged(value),
                    ))
                }
            )
            .width(Length::Fill),
            text_input("Latency", draft.network.max_depacket_latency_ms.text())
                .on_input(|value| {
                    Message::Screen(message::Screen::Settings(
                        message::screen::settings::Message::MaxDepacketLatencyMsInputChanged(value),
                    ))
                })
                .padding(8)
                .width(Length::Fixed(60.0))
                .style(theme::text_input_style),
            text("ms").size(12).style(text::secondary)
        ]
        .spacing(15)
        .align_y(Alignment::Center)
        .width(Length::Fill);
        let mut latency = column![latency].spacing(5);
        if let Some(error) = draft.network.max_depacket_latency_ms.error() {
            latency = latency.push(text(error).size(11).style(text::secondary));
        }
        let content = column![settings_section(
            lucide::network(),
            "Network",
            column![setting_row(
                "Jitter Buffer",
                "Maximum wait for out-of-order video packets (applies after restart)",
                latency,
            ),]
            .spacing(20),
        ),]
        .spacing(25);

        scrollable(content).into()
    }
}

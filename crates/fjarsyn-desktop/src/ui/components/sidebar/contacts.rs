use fjarsyn_engine::{identity::PeerId, messaging::MessageDirection, peer_session::Phase};
use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, text},
};
use iced_fonts::lucide;

use crate::ui::{
    fonts,
    message::{self, Message, Route},
    presentation::Context,
    theme,
};

pub(super) fn contact_list<'a>(
    context: Context<'a>,
    current_route: &Route,
) -> Element<'a, Message> {
    let mut peers = column![
        row![
            text("CONTACTS").size(11).style(text::secondary).font(fonts::BOLD).width(Length::Fill),
            button(lucide::user_plus().size(14))
                .on_press(Message::Navigation(message::Navigation::Navigate(Route::Contacts)))
                .style(button::text),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(8);

    if context.contacts().is_empty() {
        peers = peers.push(
            text("No trusted contacts")
                .size(12)
                .style(text::secondary)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        );
    } else {
        for contact in context.contacts() {
            let peer_id = contact.peer_id.clone();
            let target = Route::Peer { peer_id: peer_id.clone() };
            let selected = current_route.same_screen(&target);
            let nearby = context.is_nearby(&peer_id);
            let phase = context.session_for_peer(&peer_id).map(|session| session.phase);
            let subtitle = sidebar_subtitle(context, &peer_id, nearby, phase);
            let indicator = status_color(nearby, phase);

            peers = peers.push(
                button(
                    row![
                        container(lucide::user().size(16))
                            .padding(8)
                            .style(theme::icon_bubble_container),
                        column![
                            text(contact.name.clone()).size(14),
                            row![
                                container(Space::new()).width(6).height(6).style(move |_| {
                                    container::Style {
                                        background: Some(indicator.into()),
                                        border: iced::Border {
                                            radius: 3.0.into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    }
                                }),
                                text(subtitle).size(10).style(text::secondary),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        ]
                        .spacing(2),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center),
                )
                .on_press(Message::Navigation(message::Navigation::Navigate(target)))
                .width(Length::Fill)
                .style(move |theme, status| theme::sidebar_button_style(theme, status, selected)),
            );
        }
    }

    peers.into()
}

fn status_color(nearby: bool, phase: Option<Phase>) -> iced::Color {
    match phase {
        Some(Phase::Connected) => iced::Color::from_rgb(0.18, 0.72, 0.34),
        Some(Phase::Incoming | Phase::Reconnecting) => iced::Color::from_rgb(0.92, 0.66, 0.20),
        Some(Phase::Requesting | Phase::Negotiating) => iced::Color::from_rgb(0.28, 0.55, 0.92),
        Some(Phase::Disconnecting) => iced::Color::from_rgb(0.55, 0.55, 0.58),
        None if nearby => iced::Color::from_rgb(0.28, 0.55, 0.92),
        None => iced::Color::from_rgb(0.45, 0.45, 0.48),
    }
}

fn sidebar_subtitle(
    context: Context<'_>,
    peer_id: &PeerId,
    nearby: bool,
    phase: Option<Phase>,
) -> String {
    if let Some(status) = session_status(phase) {
        return status.into();
    }
    if let Some(summary) =
        context.conversation_summaries().iter().find(|summary| &summary.peer_id == peer_id)
    {
        let prefix =
            if summary.last_message_direction == MessageDirection::Outgoing { "You: " } else { "" };
        return format!(
            "{}{}",
            prefix,
            super::truncate_with_ellipsis(&summary.last_message_body, 18)
        );
    }
    if nearby { "Nearby".into() } else { "Away".into() }
}

fn session_status(phase: Option<Phase>) -> Option<&'static str> {
    match phase {
        Some(Phase::Requesting | Phase::Negotiating) => Some("Connecting"),
        Some(Phase::Incoming) => Some("Incoming request"),
        Some(Phase::Connected) => Some("Connected"),
        Some(Phase::Reconnecting) => Some("Reconnecting"),
        Some(Phase::Disconnecting) => Some("Disconnecting"),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use fjarsyn_engine::peer_session::Phase;

    use super::session_status;

    #[test]
    fn live_session_phase_takes_precedence_in_sidebar() {
        assert_eq!(session_status(Some(Phase::Incoming)), Some("Incoming request"));
        assert_eq!(session_status(Some(Phase::Negotiating)), Some("Connecting"));
        assert_eq!(session_status(Some(Phase::Connected)), Some("Connected"));
        assert_eq!(session_status(Some(Phase::Reconnecting)), Some("Reconnecting"));
        assert_eq!(session_status(Some(Phase::Disconnecting)), Some("Disconnecting"));
        assert_eq!(session_status(None), None);
    }
}

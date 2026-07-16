use fjarsyn_core::{
    communication::messaging::MessageDirection,
    pairing::PairingInvite,
    peer_session::{PeerId, PeerSessionPhase},
};
use iced::{
    Alignment, Element, Length,
    widget::{Space, button, column, container, row, scrollable, text},
};
use iced_fonts::lucide;

use crate::ui::{
    message::{Message, NavigationMessage, Route},
    shell::ShellContext,
    theme,
};

pub fn sidebar_button<'a>(
    active_route: &Route,
    target_route: Route,
    icon: iced::widget::Text<'a>,
    label: &'a str,
) -> iced::widget::Button<'a, Message> {
    let is_active = active_route.same_screen(&target_route);
    button(
        row![icon.size(16), text(label).size(14)]
            .spacing(10)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .on_press(Message::Navigation(NavigationMessage::Navigate(target_route)))
    .width(Length::Fill)
    .style(move |theme, status| theme::sidebar_button_style(theme, status, is_active))
}

pub fn sidebar<'a>(ctx: ShellContext<'a>, current_route: Route) -> Element<'a, Message> {
    let navigation = column![
        sidebar_button(&current_route, Route::Home, lucide::house(), "Home"),
        sidebar_button(&current_route, Route::Contacts, lucide::users(), "Contacts"),
    ]
    .spacing(5);

    let mut peers = column![
        row![
            text("CONTACTS")
                .size(11)
                .style(text::secondary)
                .font(crate::ui::fonts::outfit::BOLD)
                .width(Length::Fill),
            button(lucide::user_plus().size(14))
                .on_press(Message::Navigation(NavigationMessage::Navigate(Route::Contacts)))
                .style(button::text),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(8);

    if ctx.contacts.is_empty() {
        peers = peers.push(
            text("No trusted contacts")
                .size(12)
                .style(text::secondary)
                .width(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center),
        );
    } else {
        for contact in ctx.contacts.iter() {
            let peer_id = contact.peer_id.clone();
            let target = Route::Peer { peer_id: peer_id.clone() };
            let selected = current_route.same_screen(&target);
            let nearby = ctx.is_nearby(&peer_id);
            let phase = ctx.sessions.session_for_peer(&peer_id).map(|session| session.phase);
            let subtitle = sidebar_subtitle(ctx, &peer_id, nearby, phase);
            let indicator = match phase {
                Some(PeerSessionPhase::Connected) => iced::Color::from_rgb(0.18, 0.72, 0.34),
                Some(PeerSessionPhase::Incoming) => iced::Color::from_rgb(0.92, 0.66, 0.20),
                Some(PeerSessionPhase::Requesting | PeerSessionPhase::Negotiating) => {
                    iced::Color::from_rgb(0.28, 0.55, 0.92)
                }
                Some(PeerSessionPhase::Disconnecting) => iced::Color::from_rgb(0.55, 0.55, 0.58),
                None if nearby => iced::Color::from_rgb(0.28, 0.55, 0.92),
                None => iced::Color::from_rgb(0.45, 0.45, 0.48),
            };

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
                .on_press(Message::Navigation(NavigationMessage::Navigate(target)))
                .width(Length::Fill)
                .style(move |theme, status| theme::sidebar_button_style(theme, status, selected)),
            );
        }
    }

    let identity = {
        let pairing_invite =
            ctx.local_peer_id.as_ref().zip(ctx.local_public_key.as_ref()).and_then(
                |(peer_id, public_key)| {
                    PairingInvite::new(peer_id.clone(), public_key.clone()).ok()
                },
            );
        let id = ctx.local_peer_id.as_ref().map(ToString::to_string);
        let id_text = id.clone().unwrap_or_else(|| "Starting...".into());
        let display = fjarsyn_core::utils::text::truncate_with_ellipsis(&id_text, 18);
        let fingerprint = pairing_invite.as_ref().map(|invite| invite.fingerprint().to_string());
        let fingerprint_display =
            fingerprint.as_deref().map(fingerprint_grid).unwrap_or_else(|| "Starting...".into());
        let invite_text = pairing_invite.map(|invite| invite.to_string());
        let mut copy_id = button(lucide::copy().size(14)).style(button::text);
        if let Some(id) = id {
            copy_id = copy_id.on_press(Message::CopyId(id));
        }
        let mut copy_invite = button(
            row![lucide::clipboard_copy().size(14), text("Copy pairing invite").size(11)]
                .spacing(7)
                .align_y(Alignment::Center),
        )
        .style(button::text);
        if let Some(invite) = invite_text {
            copy_invite = copy_invite.on_press(Message::CopyInvite(invite));
        }
        let mut copy_fingerprint = button(lucide::copy().size(14)).style(button::text);
        if let Some(fingerprint) = fingerprint {
            copy_fingerprint = copy_fingerprint.on_press(Message::CopyFingerprint(fingerprint));
        }
        container(
            column![
                row![
                    column![
                        text("YOUR ID").size(10).style(text::secondary),
                        text(display).size(12).style(text::primary),
                    ]
                    .width(Length::Fill),
                    copy_id,
                ]
                .align_y(Alignment::Center),
                row![
                    text("FULL IDENTITY FINGERPRINT")
                        .size(10)
                        .style(text::secondary)
                        .width(Length::Fill),
                    copy_fingerprint,
                ]
                .align_y(Alignment::Center),
                text(fingerprint_display)
                    .size(12)
                    .font(iced::Font::MONOSPACE)
                    .style(text::primary)
                    .width(Length::Fill),
                copy_invite,
                text("Copying is convenient, but compare through a separate trusted channel. Pairing is mutual: import theirs too.")
                    .size(10)
                    .style(text::secondary),
            ]
            .spacing(8),
        )
        .padding(10)
        .style(theme::id_card_container)
    };

    container(
        column![
            navigation,
            container(scrollable(peers)).height(Length::Fill),
            identity,
            sidebar_button(&current_route, Route::Settings, lucide::settings(), "Settings"),
        ]
        .padding(10)
        .spacing(15),
    )
    .width(Length::Fixed(240.0))
    .height(Length::Fill)
    .style(theme::sidebar_container)
    .into()
}

fn fingerprint_grid(fingerprint: &str) -> String {
    fingerprint
        .split_whitespace()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|groups| groups.join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sidebar_subtitle(
    ctx: ShellContext<'_>,
    peer_id: &PeerId,
    nearby: bool,
    phase: Option<PeerSessionPhase>,
) -> String {
    if let Some(status) = session_status(phase) {
        return status.into();
    }
    if let Some(summary) =
        ctx.messaging.summaries.iter().find(|summary| &summary.peer_id == peer_id)
    {
        let prefix =
            if summary.last_message_direction == MessageDirection::Outgoing { "You: " } else { "" };
        return format!(
            "{}{}",
            prefix,
            fjarsyn_core::utils::text::truncate_with_ellipsis(&summary.last_message_body, 18)
        );
    }
    if nearby { "Nearby".into() } else { "Away".into() }
}

fn session_status(phase: Option<PeerSessionPhase>) -> Option<&'static str> {
    match phase {
        Some(PeerSessionPhase::Requesting | PeerSessionPhase::Negotiating) => Some("Connecting"),
        Some(PeerSessionPhase::Incoming) => Some("Incoming request"),
        Some(PeerSessionPhase::Connected) => Some("Connected"),
        Some(PeerSessionPhase::Disconnecting) => Some("Disconnecting"),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use fjarsyn_core::peer_session::PeerSessionPhase;

    use super::{fingerprint_grid, session_status};

    #[test]
    fn live_session_phase_takes_precedence_in_sidebar() {
        assert_eq!(session_status(Some(PeerSessionPhase::Incoming)), Some("Incoming request"));
        assert_eq!(session_status(Some(PeerSessionPhase::Negotiating)), Some("Connecting"));
        assert_eq!(session_status(Some(PeerSessionPhase::Connected)), Some("Connected"));
        assert_eq!(session_status(Some(PeerSessionPhase::Disconnecting)), Some("Disconnecting"));
        assert_eq!(session_status(None), None);
    }

    #[test]
    fn full_fingerprint_uses_a_fixed_readable_four_by_four_grid() {
        let fingerprint =
            "0001 0203 0405 0607 0809 0A0B 0C0D 0E0F 1011 1213 1415 1617 1819 1A1B 1C1D 1E1F";
        let grid = fingerprint_grid(fingerprint);

        assert_eq!(grid.lines().count(), 4);
        assert!(grid.lines().all(|line| line.split_whitespace().count() == 4));
        assert_eq!(grid.split_whitespace().collect::<Vec<_>>().join(" "), fingerprint);
    }
}

use fjarsyn_core::peer_session::PeerSessionPhase;
use iced::{
    Alignment, Element, Length,
    widget::{button, column, container, row, scrollable, text},
};
use iced_fonts::lucide;

use super::HomeScreen;
use crate::ui::{
    fonts,
    message::{Message, NavigationMessage, Route},
    shell::ShellContext,
    theme,
};

impl HomeScreen {
    pub fn render_view<'a>(&'a self, ctx: ShellContext<'a>) -> Element<'a, Message> {
        let nearby_contacts = ctx
            .contacts
            .iter()
            .filter_map(|contact| {
                ctx.presence
                    .is_nearby(contact.peer_id.as_str())
                    .then_some((contact, contact.peer_id.clone()))
            })
            .collect::<Vec<_>>();

        let mut nearby = column![
            row![lucide::antenna().size(20), text("Nearby contacts").size(20)]
                .spacing(10)
                .align_y(Alignment::Center),
            text("Presence is a reachability hint. A connection starts only when you request it.")
                .size(12)
                .style(text::secondary),
        ]
        .spacing(14);

        if nearby_contacts.is_empty() {
            nearby = nearby.push(
                container(
                    column![
                        lucide::radar().size(24),
                        text("No trusted contacts are nearby").size(15),
                        text("Fjarsyn is listening for local mDNS advertisements.")
                            .size(12)
                            .style(text::secondary),
                    ]
                    .spacing(8)
                    .align_x(Alignment::Center),
                )
                .padding(28)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .style(theme::card_container),
            );
        } else {
            for (contact, peer_id) in nearby_contacts {
                let session = ctx.sessions.session_for_peer(&peer_id);
                let session_label = match session.map(|session| session.phase) {
                    Some(PeerSessionPhase::Requesting | PeerSessionPhase::Negotiating) => {
                        "Connecting"
                    }
                    Some(PeerSessionPhase::Incoming) => "Incoming request",
                    Some(PeerSessionPhase::Connected) => "Connected",
                    Some(PeerSessionPhase::Disconnecting) => "Disconnecting",
                    None => "Not connected",
                };
                let target = Route::Peer { peer_id: peer_id.clone() };
                nearby = nearby.push(
                    container(
                        row![
                            container(lucide::user().size(20).center())
                                .width(44)
                                .height(44)
                                .style(theme::icon_bubble_container),
                            column![
                                text(contact.name.clone()).size(16),
                                row![
                                    text("Nearby").size(11).style(text::success),
                                    text("|").size(11).style(text::secondary),
                                    text(session_label).size(11).style(text::secondary),
                                ]
                                .spacing(6),
                            ]
                            .spacing(3)
                            .width(Length::Fill),
                            button(
                                row![text("Open").size(13), lucide::arrow_right().size(14)]
                                    .spacing(7)
                            )
                            .on_press(Message::Navigation(NavigationMessage::Navigate(target)))
                            .padding([8, 12])
                            .style(|theme, status| theme::button_style(theme, status, false)),
                        ]
                        .spacing(14)
                        .align_y(Alignment::Center),
                    )
                    .padding(14)
                    .style(theme::card_container),
                );
            }
        }

        let content = column![
            column![
                text("Home").size(32).style(text::primary).font(fonts::outfit::BOLD),
                text("Connect deliberately, then message or share your screen over WebRTC.")
                    .size(13)
                    .style(text::secondary),
                text("To add someone, both of you import the other's pairing invite and independently compare the full fingerprint.")
                    .size(12)
                    .style(text::secondary),
            ]
            .spacing(6),
            nearby,
        ]
        .spacing(28);

        container(scrollable(content)).width(Length::Fill).height(Length::Fill).padding(24).into()
    }
}

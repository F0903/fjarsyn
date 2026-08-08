use fjarsyn_engine::peer_session;
use iced::{
    Alignment, Element, Length,
    widget::{button, column, container, row, text},
};
use iced_fonts::lucide;

use super::super::Fjarsyn;
use crate::ui::{
    message::{self, Message},
    theme,
};

impl Fjarsyn {
    pub(super) fn incoming_session_popup(&self) -> Option<Element<'_, Message>> {
        let incoming = self
            .state
            .sessions
            .sessions
            .iter()
            .find(|session| session.phase == peer_session::Phase::Incoming)?;
        let name = self.state.display_name(&incoming.peer_id);
        let session_id = incoming.session_id;

        Some(
            container(
                container(
                    column![
                        text("Connection request").size(14).style(text::secondary),
                        text(name).size(20).style(text::primary),
                        text("Accept to create an authenticated WebRTC session.")
                            .size(12)
                            .style(text::secondary),
                        row![
                            button(row![lucide::check().size(15), text("Accept")].spacing(8))
                                .on_press(Message::PeerAction(message::peer::Action::Accept {
                                    session_id,
                                }))
                                .style(button::success)
                                .padding(10),
                            button(row![lucide::x().size(15), text("Reject")].spacing(8))
                                .on_press(Message::PeerAction(message::peer::Action::Reject {
                                    session_id,
                                }))
                                .style(button::danger)
                                .padding(10),
                        ]
                        .spacing(12),
                    ]
                    .spacing(14)
                    .align_x(Alignment::Center),
                )
                .padding(22)
                .style(theme::card_container),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Color { a: 0.78, ..iced::Color::BLACK }.into()),
                ..Default::default()
            })
            .into(),
        )
    }
}

use fjarsyn_core::utils::text::truncate;
use iced::{
    Element, Length,
    widget::{button, column, container, row, text},
};
use iced_fonts::lucide;

use super::super::Fjarsyn;
use crate::ui::message::{CallActionMessage, Message};

impl Fjarsyn {
    pub(super) fn incoming_call_popup<'a>(&self) -> Element<'a, Message> {
        let sender_id = match &self.ctx.session.incoming_call_id {
            Some(id) => id,
            None => {
                return column![].into();
            }
        };

        let sender_name = self
            .ctx
            .networking
            .discovered_peers
            .iter()
            .find(|p| p.id == *sender_id)
            .map(|p| p.instance_name.clone())
            .unwrap_or_else(|| format!("{}...", truncate(sender_id, 8)));

        container(
            container(
                column![
                    text("Incoming Call").size(14).style(text::secondary),
                    text(sender_name).size(20).style(text::primary),
                    row![
                        button(row![lucide::phone_incoming().size(16), text("Accept")].spacing(10))
                            .on_press(Message::CallAction(CallActionMessage::AcceptCall))
                            .style(button::success)
                            .padding(10),
                        button(row![lucide::phone_off().size(16), text("Decline")].spacing(10))
                            .on_press(Message::CallAction(CallActionMessage::DeclineCall))
                            .style(button::danger)
                            .padding(10),
                    ]
                    .spacing(15)
                ]
                .spacing(15)
                .align_x(iced::Alignment::Center),
            )
            .padding(20)
            .style(crate::ui::theme::card_container),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Color { a: 0.8, ..iced::Color::BLACK }.into()),
            ..Default::default()
        })
        .into()
    }
}

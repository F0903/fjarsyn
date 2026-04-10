use iced::{
    Alignment, Border, Color, Element, Length,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use iced_fonts::lucide;

use super::{MessagesMessage, MessagesScreen};
use crate::{
    services::messaging_service::{ConversationMessage, MessageDirection, MessageStatus},
    ui::{
        app::AppState,
        fonts,
        message::{Message, ScreenMessage},
        theme,
    },
};

impl MessagesScreen {
    pub fn render_view<'a>(&'a self, ctx: &'a AppState) -> Element<'a, Message> {
        let selected_messages = self
            .selected_peer_id
            .as_deref()
            .map(|selected_peer_id| {
                ctx.messaging
                    .messages
                    .iter()
                    .filter(|message| message.peer_id == selected_peer_id)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        container(self.view_conversation_detail(ctx, selected_messages))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }

    fn view_conversation_detail<'a>(
        &self,
        ctx: &'a AppState,
        messages: Vec<ConversationMessage>,
    ) -> Element<'a, Message> {
        let Some(selected_peer_id) = self.selected_peer_id.as_deref() else {
            return container(
                text("Choose a conversation from the sidebar or start one from Home or Contacts.")
                    .size(16)
                    .style(text::secondary),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
        };

        let title = peer_display_name(ctx, selected_peer_id);
        let truncated_peer_id = truncate_with_ellipsis(selected_peer_id, 28);

        let mut transcript = column![].spacing(12);
        if messages.is_empty() {
            transcript = transcript.push(
                container(text("No messages yet.").size(14).style(text::secondary))
                    .padding(16)
                    .style(crate::ui::theme::card_container)
                    .width(Length::Fill),
            );
        } else {
            for message in messages {
                transcript = transcript.push(self.view_message_bubble(message));
            }
        }

        let composer = row![
            text_input("Type a message...", &self.draft)
                .on_input(|value| Message::Screen(ScreenMessage::Messages(
                    MessagesMessage::DraftChanged(value),
                )))
                .on_submit(Message::Screen(ScreenMessage::Messages(MessagesMessage::SendPressed)))
                .padding(12)
                .style(theme::text_input_style)
                .width(Length::Fill),
            button(row![lucide::send().size(16), text("Send")].spacing(8))
                .on_press(Message::Screen(ScreenMessage::Messages(MessagesMessage::SendPressed)))
                .padding(12)
                .style(|theme, status| theme::button_style(theme, status, true)),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        column![
            row![
                column![
                    text(title).size(30).style(text::primary).font(fonts::outfit::BOLD),
                    text(truncated_peer_id).size(12).style(text::secondary),
                ]
                .spacing(4)
                .width(Length::Fill)
            ]
            .align_y(Alignment::Center),
            container(scrollable(transcript))
                .height(Length::Fill)
                .width(Length::Fill)
                .padding(16)
                .style(crate::ui::theme::card_container),
            composer,
        ]
        .spacing(16)
        .height(Length::Fill)
        .into()
    }

    fn view_message_bubble<'a>(&self, message: ConversationMessage) -> Element<'a, Message> {
        let is_outgoing = matches!(message.direction, MessageDirection::Outgoing);
        let status_icon = match message.status {
            MessageStatus::Pending => lucide::minus().size(12),
            MessageStatus::Delivered => lucide::check().size(12),
            MessageStatus::Failed => lucide::x().size(12),
        };

        let bubble = container(
            column![
                text(message.body.clone()).size(14),
                row![
                    text(message.created_at.format("%H:%M").to_string())
                        .size(11)
                        .style(text::secondary),
                    status_icon.style(text::secondary),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
            ]
            .spacing(8),
        )
        .padding([9, 11])
        .max_width(360)
        .style(move |_| container::Style {
            background: Some(if is_outgoing {
                Color { a: 0.18, ..crate::ui::theme::PRIMARY_COLOR }.into()
            } else {
                crate::ui::theme::CARD_BACKGROUND.into()
            }),
            border: Border {
                color: crate::ui::theme::BORDER_COLOR,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        });

        if is_outgoing {
            row![container(column![].width(Length::Fill)).width(Length::Fill), bubble]
                .width(Length::Fill)
                .into()
        } else {
            row![bubble, container(column![].width(Length::Fill)).width(Length::Fill)]
                .width(Length::Fill)
                .into()
        }
    }
}

fn peer_display_name(ctx: &AppState, peer_id: &str) -> String {
    if let Some(contact) = ctx.services.contacts_service.as_ref().and_then(|service| {
        service.contacts().iter().find(|contact| contact.peer_id == peer_id).cloned()
    }) {
        return truncate_with_ellipsis(contact.name.trim(), 28);
    }

    ctx.networking
        .discovered_peers
        .iter()
        .find(|peer| peer.id == peer_id)
        .map(|peer| peer.instance_name.trim().to_string())
        .filter(|name| !name.is_empty())
        .map(|name| truncate_with_ellipsis(&name, 28))
        .unwrap_or_else(|| crate::utils::string_utils::abbreviate_middle(peer_id, 14, 6))
}

fn truncate_with_ellipsis(value: &str, max_chars: usize) -> String {
    crate::utils::string_utils::truncate_with_ellipsis(value, max_chars)
}

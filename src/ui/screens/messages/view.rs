use iced::{
    Alignment, Element, Length,
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
            .and_then(|selected_peer_id| {
                ctx.services.messaging_service.as_ref().map(|service| {
                    service
                        .messages()
                        .iter()
                        .filter(|message| message.peer_id == selected_peer_id)
                        .cloned()
                        .collect::<Vec<_>>()
                })
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
        let status_label = peer_status_label(ctx, selected_peer_id);

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
                    text(selected_peer_id.to_string()).size(12).style(text::secondary),
                ]
                .spacing(4)
                .width(Length::Fill),
                text(status_label).size(12).style(text::secondary)
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
        let status_label = match message.status {
            MessageStatus::Pending => "Pending receipt",
            MessageStatus::Delivered => "Delivered",
            MessageStatus::Failed => "Failed",
        };

        let bubble = container(
            column![
                text(message.body.clone()).size(14),
                row![
                    text(message.created_at.format("%H:%M").to_string())
                        .size(11)
                        .style(text::secondary),
                    text(status_label).size(11).style(text::secondary),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
            ]
            .spacing(10),
        )
        .padding(14)
        .max_width(420)
        .style(if is_outgoing {
            crate::ui::theme::icon_bubble_container
        } else {
            crate::ui::theme::card_container
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
        return contact.name;
    }

    ctx.networking
        .discovered_peers
        .iter()
        .find(|peer| peer.id == peer_id)
        .map(|peer| peer.instance_name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| crate::utils::string_utils::truncate(peer_id, 12).to_string())
}

fn peer_status_label(ctx: &AppState, peer_id: &str) -> String {
    if ctx.networking.discovered_peers.iter().any(|peer| peer.id == peer_id) {
        return "Reachable".into();
    }

    if let Some(contact) = ctx.services.contacts_service.as_ref().and_then(|service| {
        service.contacts().iter().find(|contact| contact.peer_id == peer_id).cloned()
    }) && contact.address.as_deref().is_some_and(|address| !address.trim().is_empty())
    {
        return "Saved route".into();
    }

    "Offline".into()
}

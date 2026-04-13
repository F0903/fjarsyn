use std::sync::Arc;

use iced::Task;

use crate::ui::{
    app::Fjarsyn,
    message::{Message, MessagingServiceMessage, ScreenMessage},
    screens::messages::MessagesMessage,
};

pub(super) fn run_send_message(
    app: &mut Fjarsyn,
    peer_id: String,
    address: std::net::SocketAddr,
    body: String,
) -> Task<Message> {
    let Some(service) = app.runtime.services.messaging_service.clone() else {
        return Task::none();
    };

    Task::future(async move {
        Message::Messaging(MessagingServiceMessage::MessageSent(
            service.send_message(peer_id, address, body).await.map_err(Arc::new),
        ))
    })
}

pub(super) fn run_refresh_active_conversation(app: &mut Fjarsyn) -> Task<Message> {
    let messages = match (
        app.runtime.services.messaging_service.as_ref(),
        app.ctx.messaging.active_peer_id.as_deref(),
    ) {
        (Some(service), Some(peer_id)) => service.messages_for_peer(peer_id),
        _ => Arc::new(Vec::new()),
    };

    Task::done(Message::Messaging(MessagingServiceMessage::ActiveConversationLoaded(messages)))
}

pub(super) fn run_clear_message_draft(peer_id: String) -> Task<Message> {
    Task::done(Message::Screen(ScreenMessage::Messages(MessagesMessage::ClearDraft(peer_id))))
}

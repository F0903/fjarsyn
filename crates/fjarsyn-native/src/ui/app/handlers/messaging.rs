use std::sync::Arc;

use fjarsyn_core::{
    app::{self, MessagingAction},
    services::messaging_service::MessagingEvent,
};
use iced::Task;

use crate::ui::{
    app::{Fjarsyn, handlers::app_command},
    message::{Message, MessagingServiceMessage},
};

pub fn handle_messaging_msg(app: &mut Fjarsyn, message: MessagingServiceMessage) -> Task<Message> {
    let action = match message {
        MessagingServiceMessage::ServiceInitialized(result) => match result {
            Ok(service) => {
                let summaries = service.conversation_summaries();
                app.runtime.services.messaging_service = Some(service);
                MessagingAction::ServiceReady { summaries }
            }
            Err(err) => MessagingAction::ServiceInitFailed(err.to_string()),
        },
        MessagingServiceMessage::Event(event) => messaging_event_action(app, event),
        MessagingServiceMessage::SendMessage { peer_id, address, body } => {
            MessagingAction::SendRequested { peer_id, address, body }
        }
        MessagingServiceMessage::MessageSent(result) => {
            MessagingAction::SendCompleted(result.map_err(|err| err.to_string()))
        }
        MessagingServiceMessage::ActiveConversationLoaded(messages) => {
            MessagingAction::ActiveConversationLoaded(messages)
        }
    };

    let commands = app::reduce_messaging(&mut app.ctx.core, action);
    app_command::run_app_commands(app, commands)
}

pub fn sync_active_conversation(app: &mut Fjarsyn, peer_id: Option<String>) -> Task<Message> {
    let commands = app::reduce_messaging(
        &mut app.ctx.core,
        MessagingAction::ActiveConversationSelected(peer_id),
    );
    app_command::run_app_commands(app, commands)
}

fn messaging_event_action(app: &Fjarsyn, event: MessagingEvent) -> MessagingAction {
    let summaries = app
        .runtime
        .services
        .messaging_service
        .as_ref()
        .map(|service| service.conversation_summaries())
        .unwrap_or_else(|| Arc::new(Vec::new()));

    MessagingAction::Event { event, summaries }
}

use std::sync::Arc;

use super::{AppCommands, notify_error, notify_info};
use crate::{
    app::{
        AppCommand, AppState, ServicePhase, message_preview, peer_label, resolve_selected_peer_id,
    },
    communication::messaging::{ConversationMessage, ConversationSummary, MessagingEvent},
};

#[derive(Debug, Clone)]
pub enum MessagingAction {
    ServiceReady { summaries: Arc<Vec<ConversationSummary>> },
    ServiceInitFailed(String),
    SendRequested { peer_id: String, address: std::net::SocketAddr, body: String },
    SendCompleted(Result<String, String>),
    Event { event: MessagingEvent, summaries: Arc<Vec<ConversationSummary>> },
    ActiveConversationSelected(Option<String>),
    ActiveConversationLoaded(Arc<Vec<ConversationMessage>>),
}

pub fn execute_messaging_action(state: &mut AppState, action: MessagingAction) -> AppCommands {
    match action {
        MessagingAction::ServiceReady { summaries } => {
            state.services.messaging = ServicePhase::Ready;
            update_messaging_summaries(state, summaries);
            ensure_active_conversation(state)
        }
        MessagingAction::ServiceInitFailed(err) => {
            state.services.messaging = ServicePhase::Failed;
            smallvec::smallvec![notify_error(format!(
                "Messaging service failed to initialize: {}",
                err
            ))]
        }
        MessagingAction::SendRequested { peer_id, address, body } => {
            if !state.can_use_messaging() {
                return smallvec::smallvec![notify_error(messaging_unavailable_message(state))];
            }

            smallvec::smallvec![AppCommand::SendMessage { peer_id, address, body }]
        }
        MessagingAction::SendCompleted(result) => match result {
            Ok(peer_id) => smallvec::smallvec![AppCommand::ClearMessageDraft(peer_id)],
            Err(err) => smallvec::smallvec![notify_error(err)],
        },
        MessagingAction::Event { event, summaries } => {
            execute_messaging_event(state, event, summaries)
        }
        MessagingAction::ActiveConversationSelected(peer_id) => {
            state.messaging.active_peer_id = peer_id;
            if state.can_use_messaging() {
                smallvec::smallvec![AppCommand::RefreshActiveConversation]
            } else {
                AppCommands::new()
            }
        }
        MessagingAction::ActiveConversationLoaded(messages) => {
            state.messaging.active_messages = messages;
            AppCommands::new()
        }
    }
}

fn ensure_active_conversation(state: &mut AppState) -> AppCommands {
    if state.messaging.active_peer_id.is_none() {
        state.messaging.active_peer_id = resolve_selected_peer_id(
            &state.messaging.summaries,
            state.messaging.active_peer_id.clone(),
        );
    }

    smallvec::smallvec![AppCommand::RefreshActiveConversation]
}

fn execute_messaging_event(
    state: &mut AppState,
    event: MessagingEvent,
    summaries: Arc<Vec<ConversationSummary>>,
) -> AppCommands {
    update_messaging_summaries(state, summaries);

    match event {
        MessagingEvent::ConversationUpdated { peer_id } => {
            let mut commands = maybe_select_first_conversation(state, &peer_id);
            if state.messaging.active_peer_id.as_deref() == Some(peer_id.as_str()) {
                commands.push(AppCommand::RefreshActiveConversation);
            }
            commands
        }
        MessagingEvent::IncomingMessage { peer_id, body } => {
            let mut commands = maybe_select_first_conversation(state, &peer_id);
            if state.messaging.active_peer_id.as_deref() == Some(peer_id.as_str()) {
                commands.push(AppCommand::RefreshActiveConversation);
            } else {
                commands.push(notify_info(format!(
                    "New message from {}: {}",
                    peer_label(
                        &state.contacts.contacts,
                        &state.networking.discovered_peers,
                        &peer_id,
                    ),
                    message_preview(&body, 32)
                )));
            }
            commands
        }
    }
}

fn maybe_select_first_conversation(state: &mut AppState, peer_id: &str) -> AppCommands {
    if state.messaging.active_peer_id.is_none() {
        state.messaging.active_peer_id = Some(peer_id.to_string());
        smallvec::smallvec![AppCommand::RefreshActiveConversation]
    } else {
        AppCommands::new()
    }
}

fn update_messaging_summaries(state: &mut AppState, summaries: Arc<Vec<ConversationSummary>>) {
    state.messaging.summaries = summaries;
    state.messaging.revision = state.messaging.revision.wrapping_add(1);
}

fn messaging_unavailable_message(state: &AppState) -> &'static str {
    if !state.accepts_user_requests() {
        "Messaging is unavailable while the app is shutting down."
    } else {
        "Messaging is unavailable until the service is ready."
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;

    use super::*;
    use crate::{
        app::NotificationLevel,
        communication::messaging::{MessageDirection, MessageStatus},
        executors::test_support::state,
    };

    #[test]
    fn incoming_message_selects_first_conversation_and_refreshes() {
        let mut state = state();
        let summaries = Arc::new(vec![ConversationSummary {
            peer_id: "peer-a".into(),
            last_message_id: 1,
            last_message_body: "hello".into(),
            last_message_direction: MessageDirection::Incoming,
            last_message_status: MessageStatus::Delivered,
            last_message_at: Utc::now(),
        }]);

        let commands = execute_messaging_action(
            &mut state,
            MessagingAction::Event {
                event: MessagingEvent::IncomingMessage {
                    peer_id: "peer-a".into(),
                    body: "hello".into(),
                },
                summaries,
            },
        );

        assert_eq!(state.messaging.active_peer_id.as_deref(), Some("peer-a"));
        assert!(
            commands.iter().any(|command| matches!(command, AppCommand::RefreshActiveConversation))
        );
    }

    #[test]
    fn active_conversation_load_replaces_messages() {
        let mut state = state();
        let messages = Arc::new(vec![ConversationMessage {
            id: 1,
            message_id: "msg-1".into(),
            peer_id: "peer-a".into(),
            direction: MessageDirection::Incoming,
            body: "hello".into(),
            status: MessageStatus::Delivered,
            created_at: Utc::now(),
            delivered_at: None,
        }]);

        let commands = execute_messaging_action(
            &mut state,
            MessagingAction::ActiveConversationLoaded(messages.clone()),
        );

        assert!(commands.is_empty());
        assert_eq!(state.messaging.active_messages.len(), 1);
        assert_eq!(state.messaging.active_messages[0].body, "hello");
    }

    #[test]
    fn sending_message_requires_ready_messaging_service() {
        let mut state = state();

        let commands = execute_messaging_action(
            &mut state,
            MessagingAction::SendRequested {
                peer_id: "peer-a".into(),
                address: "127.0.0.1:9000".parse().unwrap(),
                body: "hello".into(),
            },
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::Notify { level: NotificationLevel::Error, .. }
        )));
    }
}

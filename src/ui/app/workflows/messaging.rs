use std::{net::SocketAddr, sync::Arc};

use crate::{
    services::messaging_service::{MessagingEvent, MessagingService},
    ui::{
        app::{ActiveScreen, Fjarsyn},
        message::{MessagingServiceMessage, Route},
    },
};

pub(crate) enum MessagingEffect {
    NotifyError(String),
    NotifyInfo(String),
    Navigate(Route),
    SendMessage {
        service: Arc<MessagingService>,
        peer_id: String,
        address: SocketAddr,
        body: String,
    },
}

pub(crate) fn reduce(app: &mut Fjarsyn, message: MessagingServiceMessage) -> Vec<MessagingEffect> {
    match message {
        MessagingServiceMessage::Initialize => Vec::new(),
        MessagingServiceMessage::ServiceInitialized(result) => match result {
            Ok(service) => {
                app.ctx.services.messaging_service = Some(service);
                Vec::new()
            }
            Err(err) => vec![MessagingEffect::NotifyError(format!(
                "Messaging service failed to initialize: {}",
                err
            ))],
        },
        MessagingServiceMessage::Event(event) => reduce_event(app, event),
        MessagingServiceMessage::OpenConversation(peer_id) => {
            app.ctx.messaging.pending_open_peer_id = Some(peer_id);
            vec![MessagingEffect::Navigate(Route::Messages)]
        }
        MessagingServiceMessage::SendMessage { peer_id, address, body } => app
            .ctx
            .services
            .messaging_service
            .clone()
            .map(|service| MessagingEffect::SendMessage { service, peer_id, address, body })
            .into_iter()
            .collect(),
    }
}

fn reduce_event(app: &mut Fjarsyn, event: MessagingEvent) -> Vec<MessagingEffect> {
    match event {
        MessagingEvent::ConversationUpdated { .. } => Vec::new(),
        MessagingEvent::IncomingMessage { peer_id, body } => {
            let is_active_conversation = matches!(
                &app.active_screen,
                ActiveScreen::Messages(screen)
                    if screen.selected_peer_id.as_deref() == Some(peer_id.as_str())
            );

            if is_active_conversation {
                Vec::new()
            } else {
                vec![MessagingEffect::NotifyInfo(format!(
                    "New message from {}: {}",
                    peer_label(app, &peer_id),
                    preview(&body)
                ))]
            }
        }
    }
}

fn peer_label(app: &Fjarsyn, peer_id: &str) -> String {
    if let Some(contacts) = app.ctx.services.contacts_service.as_ref()
        && let Some(contact) = contacts.contacts().iter().find(|contact| contact.peer_id == peer_id)
    {
        return contact.name.clone();
    }

    app.ctx
        .networking
        .discovered_peers
        .iter()
        .find(|peer| peer.id == peer_id)
        .map(|peer| peer.instance_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| crate::utils::string_utils::truncate(peer_id, 12).to_string())
}

fn preview(body: &str) -> String {
    if body.chars().count() <= 32 {
        body.to_string()
    } else {
        format!("{}...", body.chars().take(32).collect::<String>())
    }
}

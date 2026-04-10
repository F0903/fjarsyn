use std::{net::SocketAddr, sync::Arc};

use crate::{
    services::messaging_service::{MessagingEvent, MessagingService},
    ui::{
        app::{ActiveScreen, Fjarsyn},
        message::MessagingServiceMessage,
    },
};

pub(crate) enum MessagingEffect {
    NotifyError(String),
    NotifyInfo(String),
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
                app.ctx.messaging.messages = service.messages();
                app.ctx.services.messaging_service = Some(service);
                Vec::new()
            }
            Err(err) => vec![MessagingEffect::NotifyError(format!(
                "Messaging service failed to initialize: {}",
                err
            ))],
        },
        MessagingServiceMessage::Event(event) => reduce_event(app, event),
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
        MessagingEvent::ConversationUpdated { peer_id } => {
            if let Some(service) = app.ctx.services.messaging_service.as_ref() {
                app.ctx.messaging.messages = service.messages();
            }

            app.ctx.messaging.revision = app.ctx.messaging.revision.wrapping_add(1);

            if let ActiveScreen::Messages(screen) = &mut app.active_screen
                && screen.selected_peer_id.is_none()
            {
                screen.selected_peer_id = Some(peer_id);
            }

            Vec::new()
        }
        MessagingEvent::IncomingMessage { peer_id, body } => {
            if let Some(service) = app.ctx.services.messaging_service.as_ref() {
                app.ctx.messaging.messages = service.messages();
            }

            app.ctx.messaging.revision = app.ctx.messaging.revision.wrapping_add(1);

            let is_active_conversation = match &mut app.active_screen {
                ActiveScreen::Messages(screen) => {
                    if screen.selected_peer_id.is_none() {
                        screen.selected_peer_id = Some(peer_id.clone());
                    }

                    screen.selected_peer_id.as_deref() == Some(peer_id.as_str())
                }
                _ => false,
            };

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

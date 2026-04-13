use std::sync::Arc;

use fjarsyn_core::{
    repositories::MessagesRepository,
    services::{
        discovery_service::DiscoveryService,
        messaging_service::{MessagingService, MessagingServiceConfig},
    },
};
use iced::Task;

use crate::ui::{
    message::{CallServiceMessage, Message, MessagingServiceMessage},
    shell::Fjarsyn,
};

pub(super) fn run_retry_startup(app: &mut Fjarsyn) -> Task<Message> {
    app.runtime.db = None;
    app.runtime.services.call_service = None;
    app.runtime.services.contacts_service = None;
    app.runtime.services.discovery_service = None;
    app.runtime.services.messaging_service = None;

    Fjarsyn::startup_service_tasks(app)
}

pub(super) fn run_initialize_discovery(
    app: &mut Fjarsyn,
    local_peer_id: String,
    signaling_port: u16,
) -> Task<Message> {
    let event_tx = app.runtime.discovery_event_tx.clone();
    Task::future(async move {
        Message::CallService(CallServiceMessage::DiscoveryServiceInitialized(
            DiscoveryService::init(local_peer_id, signaling_port, event_tx)
                .map(Arc::new)
                .map_err(Arc::new),
        ))
    })
}

pub(super) fn run_initialize_messaging(app: &mut Fjarsyn) -> Task<Message> {
    let Some(db) = app.runtime.db.clone() else {
        return Task::none();
    };
    let Some(call_service) = app.runtime.services.call_service.clone() else {
        return Task::none();
    };
    let event_tx = app.runtime.messaging_event_tx.clone();

    Task::future(async move {
        Message::Messaging(MessagingServiceMessage::ServiceInitialized(
            MessagingService::init(MessagingServiceConfig {
                repository: Arc::new(MessagesRepository::new(db)),
                webrtc: call_service.webrtc(),
                event_tx,
            })
            .await
            .map(Arc::new)
            .map_err(Arc::new),
        ))
    })
}

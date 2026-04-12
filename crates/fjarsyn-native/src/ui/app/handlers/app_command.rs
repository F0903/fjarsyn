use std::sync::Arc;

use fjarsyn_core::{
    app::{AppCommand, NotificationLevel},
    discovery::PeerInfo,
    repositories::MessagesRepository,
    services::{
        call_service::DialSuccess,
        discovery_service::DiscoveryService,
        messaging_service::{MessagingService, MessagingServiceConfig},
    },
};
use iced::Task;

use crate::ui::{
    app::Fjarsyn,
    message::{
        CallServiceMessage, ContactsServiceMessage, Message, MessagingServiceMessage,
        NavigationMessage, ScreenMessage,
    },
    screens::messages::MessagesMessage,
    utils::ErrorExt,
};

pub fn run_app_commands(app: &mut Fjarsyn, commands: Vec<AppCommand>) -> Task<Message> {
    Task::batch(commands.into_iter().map(|command| run_app_command(app, command)))
}

pub fn run_app_command(app: &mut Fjarsyn, command: AppCommand) -> Task<Message> {
    match command {
        AppCommand::Notify { level, message } => {
            run_notification(app, level, message);
            Task::none()
        }
        AppCommand::SaveConfig => {
            if let Err(err) = app.ctx.config.save() {
                app.ctx.notify_error(format!("Failed to save peer ID: {}", err));
            }
            Task::none()
        }
        AppCommand::Navigate(route) => {
            Task::done(Message::Navigation(NavigationMessage::Navigate(route)))
        }
        AppCommand::LoadContacts => {
            let Some(service) = app.runtime.services.contacts_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                Message::ContactData(ContactsServiceMessage::ContactsLoaded(
                    match service.refresh().await {
                        Ok(()) => Ok(service.contacts()),
                        Err(err) => Err(err),
                    },
                ))
            })
        }
        AppCommand::SaveContact { peer_id, name, address } => {
            let Some(service) = app.runtime.services.contacts_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                let result = match service.create(peer_id, name, address).await {
                    Ok(_) => Ok(service.contacts()),
                    Err(err) => Err(err),
                };
                Message::ContactData(ContactsServiceMessage::ContactSaved(result))
            })
        }
        AppCommand::DeleteContact { id } => {
            let Some(service) = app.runtime.services.contacts_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                let result = match service.delete(id).await {
                    Ok(()) => Ok(service.contacts()),
                    Err(err) => Err(err),
                };
                Message::ContactData(ContactsServiceMessage::ContactDeleted(result))
            })
        }
        AppCommand::UpdateContactAddress { id, peer_id, name, address } => {
            let Some(service) = app.runtime.services.contacts_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                let result = match service.update(id, peer_id, name, Some(address)).await {
                    Ok(()) => Ok(service.contacts()),
                    Err(err) => Err(err),
                };
                Message::ContactData(ContactsServiceMessage::ContactUpdated(result))
            })
        }
        AppCommand::AcceptCall => {
            let Some(service) = app.runtime.services.call_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                service
                    .accept()
                    .await
                    .map(|_| {
                        Message::Navigation(NavigationMessage::Navigate(
                            crate::ui::message::Route::Call,
                        ))
                    })
                    .unwrap_or_else(|err| err.to_notify_error())
            })
        }
        AppCommand::DeclineCall => {
            let Some(service) = app.runtime.services.call_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                service
                    .decline()
                    .await
                    .map(|_| Message::NoOp)
                    .unwrap_or_else(|err| err.to_notify_error())
            })
        }
        AppCommand::StartCall { target } => {
            let Some(service) = app.runtime.services.call_service.clone() else {
                return Task::none();
            };
            let contacts = app.ctx.contacts.contacts.clone();
            let discovered = app.ctx.networking.discovered_peers.clone();

            Task::future(async move {
                service
                    .dial(target, &contacts, &discovered)
                    .await
                    .map(|DialSuccess { peer_id, name, socket_addr, update_contact_address }| {
                        let mut batch = Vec::new();

                        if let Some((id, addr)) = update_contact_address {
                            batch.push(Message::ContactData(
                                ContactsServiceMessage::UpdateContactAddress {
                                    id,
                                    new_address: addr,
                                },
                            ));
                        }

                        if let (Some(id), Some(name), Some(addr)) = (peer_id, name, socket_addr) {
                            batch.push(Message::CallService(CallServiceMessage::PeerFound(
                                PeerInfo {
                                    id,
                                    instance_name: name,
                                    host_name: "direct".into(),
                                    addresses: vec![addr.ip()],
                                    port: addr.port(),
                                },
                            )));
                        }

                        batch.push(Message::Navigation(NavigationMessage::Navigate(
                            crate::ui::message::Route::Call,
                        )));
                        Message::Batch(batch)
                    })
                    .unwrap_or_else(|err| err.to_notify_error())
            })
        }
        AppCommand::SendMessage { peer_id, address, body } => {
            let Some(service) = app.runtime.services.messaging_service.clone() else {
                return Task::none();
            };

            Task::future(async move {
                Message::Messaging(MessagingServiceMessage::MessageSent(
                    service.send_message(peer_id, address, body).await.map_err(Arc::new),
                ))
            })
        }
        AppCommand::InitializeDiscovery { local_peer_id, signaling_port } => {
            let event_tx = app.runtime.discovery_event_tx.clone();
            Task::future(async move {
                Message::CallService(CallServiceMessage::DiscoveryServiceInitialized(
                    DiscoveryService::init(local_peer_id, signaling_port, event_tx)
                        .map(Arc::new)
                        .map_err(Arc::new),
                ))
            })
        }
        AppCommand::InitializeMessaging => {
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
        AppCommand::RefreshActiveConversation => {
            let messages = match (
                app.runtime.services.messaging_service.as_ref(),
                app.ctx.messaging.active_peer_id.as_deref(),
            ) {
                (Some(service), Some(peer_id)) => service.messages_for_peer(peer_id),
                _ => Arc::new(Vec::new()),
            };

            Task::done(Message::Messaging(MessagingServiceMessage::ActiveConversationLoaded(
                messages,
            )))
        }
        AppCommand::ClearMessageDraft(peer_id) => Task::done(Message::Screen(
            ScreenMessage::Messages(MessagesMessage::ClearDraft(peer_id)),
        )),
    }
}

fn run_notification(app: &mut Fjarsyn, level: NotificationLevel, message: String) {
    match level {
        NotificationLevel::Error => app.ctx.notify_error(message),
        NotificationLevel::Info => app.ctx.notify_info(message),
        NotificationLevel::Success => app.ctx.notify_success(message),
    }
}

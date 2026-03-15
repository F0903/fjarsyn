use std::sync::Arc;

use iced::Task;

use crate::{
    networking::discovery::DiscoveryEvent,
    services::{call_service::CallEvent, contacts_service::ContactsService},
    ui::{
        app::Fjarsyn,
        message::{
            CallServiceMessage, CaptureMessage, ContactsServiceMessage, DatabaseMessage, Message,
            NavigationMessage, Route,
        },
    },
};

pub fn handle_call_service_msg(app: &mut Fjarsyn, msg: CallServiceMessage) -> Task<Message> {
    match msg {
        CallServiceMessage::CallServiceInitialized(res) => {
            if let Ok(service) = res {
                if app.ctx.config.peer_id.is_none() {
                    app.ctx.config.peer_id = Some(service.local_id().to_string());
                    let _ = app.ctx.config.save();
                }
                app.ctx.services.call_service = Some(service.clone());
            }
            Task::none()
        }
        CallServiceMessage::CallEvent(event) => {
            match event {
                CallEvent::IncomingCall { peer_id } => {
                    app.ctx.session.target_id = Some(peer_id.clone());
                    app.ctx.session.incoming_call_id = Some(peer_id.clone());
                    app.ctx.session.incoming_call_timeout =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
                }
                CallEvent::CallConnected => {
                    app.ctx.session.incoming_call_id = None;
                    app.ctx.session.incoming_call_timeout = None;
                    if let Some(tid) = &app.ctx.session.target_id
                        && let Some(p) = app
                            .ctx
                            .networking
                            .discovered_peers
                            .iter()
                            .find(|p| p.id == *tid)
                            .cloned()
                        {
                            app.ctx.networking.recent_peers.retain(|rp| rp.id != p.id);
                            app.ctx.networking.recent_peers.insert(0, p);
                        }
                    return Task::done(Message::Navigation(NavigationMessage::Navigate(
                        Route::Call,
                    )));
                }
                CallEvent::CallEnded => {
                    if app.ctx.session.target_id.is_some() {
                        app.ctx.notify_info("Call ended.");
                    }
                    app.ctx.session.target_id = None;
                    app.ctx.session.incoming_call_id = None;
                    app.ctx.session.incoming_call_timeout = None;
                }
            }
            Task::none()
        }
        CallServiceMessage::DiscoveryEvent(event) => {
            match event {
                DiscoveryEvent::PeerFound(peer) => {
                    // Don't add ourselves
                    if app
                        .ctx
                        .services
                        .call_service
                        .as_ref()
                        .map(|s| s.local_id() == peer.id)
                        .unwrap_or(false)
                    {
                        return Task::none();
                    }

                    if let Some(existing) =
                        app.ctx.networking.discovered_peers.iter_mut().find(|p| p.id == peer.id)
                    {
                        existing.update(peer.clone());
                    } else {
                        app.ctx.networking.discovered_peers.push(peer.clone());
                    }
                }
                DiscoveryEvent::PeerRemoved(fullname) => {
                    app.ctx
                        .networking
                        .discovered_peers
                        .retain(|p| !fullname.contains(&p.instance_name));
                }
            }
            Task::none()
        }
        CallServiceMessage::PeerFound(peer) => {
            // Return if the peer is us
            if app
                .ctx
                .services
                .call_service
                .as_ref()
                .map(|s| s.local_id() == peer.id)
                .unwrap_or(false)
            {
                return Task::none();
            }

            if let Some(existing) =
                app.ctx.networking.discovered_peers.iter_mut().find(|p| p.id == peer.id)
            {
                existing.update(peer.clone());
            } else {
                app.ctx.networking.discovered_peers.push(peer.clone());
            }
            Task::none()
        }
        CallServiceMessage::PeerRemoved(id) => {
            app.ctx.networking.discovered_peers.retain(|p| p.id != id);
            Task::none()
        }
        CallServiceMessage::PacketReceived(_) => Task::none(),
    }
}

pub fn handle_capture_msg(app: &mut Fjarsyn, msg: CaptureMessage) -> Task<Message> {
    match msg {
        CaptureMessage::CaptureInitialized(res) => match res {
            Ok(provider) => {
                app.ctx.media.capture = Some(provider);
                tracing::info!("Capture ready.");
            }
            Err(e) => {
                app.ctx.notify_error(format!("Capture Failed: {}", e));
            }
        },
    }
    Task::none()
}

pub fn handle_database_msg(app: &mut Fjarsyn, msg: DatabaseMessage) -> Task<Message> {
    match msg {
        DatabaseMessage::DatabaseInitialized(res) => match res {
            Ok(pool) => {
                app.ctx.db = Some(pool.clone());
                app.ctx.services.contacts_service =
                    Some(Arc::new(ContactsService::new(pool.clone())));
                Task::done(Message::ContactData(ContactsServiceMessage::LoadContacts))
            }
            Err(e) => {
                app.ctx.notify_error(format!("DB Failed: {}", e));
                Task::none()
            }
        },
    }
}

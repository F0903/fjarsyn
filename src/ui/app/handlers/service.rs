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
        screens::ActiveScreen,
    },
};

fn upsert_discovered_peer(app: &mut Fjarsyn, peer: crate::networking::discovery::PeerInfo) {
    if let Some(existing) = app.ctx.networking.discovered_peers.iter_mut().find(|p| p.id == peer.id)
    {
        existing.update(peer);
    } else {
        app.ctx.networking.discovered_peers.push(peer);
    }
}

pub fn handle_call_service_msg(app: &mut Fjarsyn, msg: CallServiceMessage) -> Task<Message> {
    match msg {
        CallServiceMessage::CallServiceInitialized(res) => {
            match res {
                Ok(service) => {
                    if app.ctx.config.peer_id.is_none() {
                        app.ctx.config.peer_id = Some(service.local_id().to_string());
                        if let Err(err) = app.ctx.config.save() {
                            app.ctx.notify_error(format!("Failed to save peer ID: {}", err));
                        }
                    }
                    app.ctx.services.call_service = Some(service);
                }
                Err(err) => {
                    app.ctx.notify_error(format!("Call service failed to initialize: {}", err));
                }
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

                    upsert_discovered_peer(app, peer);
                }
                DiscoveryEvent::PeerRemoved(fullname) => {
                    app.ctx.networking.discovered_peers.retain(|p| p.instance_name != fullname);
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

            upsert_discovered_peer(app, peer);
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
        CaptureMessage::CaptureInitialized(res) => {
            app.ctx.media.capture_initializing = false;

            match res {
                Ok(provider) => {
                    let provider_for_screen = provider.clone();
                    let retry_start_capture =
                        if let ActiveScreen::Call(screen) = &mut app.active_screen {
                            screen.capture = Some(provider_for_screen);
                            if screen.pending_capture_start {
                                screen.pending_capture_start = false;
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                    app.ctx.media.capture = Some(provider);
                    tracing::info!("Capture ready.");

                    if retry_start_capture {
                        use crate::ui::message::{Message, ScreenMessage};
                        return Task::done(Message::Screen(ScreenMessage::Call(
                            crate::ui::screens::call::CallMessage::StartCapture,
                        )));
                    }
                }
                Err(e) => {
                    if let ActiveScreen::Call(screen) = &mut app.active_screen {
                        screen.pending_capture_start = false;
                    }
                    app.ctx.notify_error(format!("Capture Failed: {}", e));
                }
            }
            Task::none()
        }
    }
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

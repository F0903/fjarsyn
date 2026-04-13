use std::sync::Arc;

use fjarsyn_core::{
    executors::{AppEvent, ServiceAction},
    repositories::ContactsRepository,
    services::contacts_service::ContactsService,
};
use iced::Task;

use crate::ui::{
    app::{ActiveScreen, Fjarsyn, handlers::app_event},
    message::{CallServiceMessage, CaptureMessage, DatabaseMessage, Message, ScreenMessage},
};

pub fn handle_call_service_msg(app: &mut Fjarsyn, message: CallServiceMessage) -> Task<Message> {
    let action = match message {
        CallServiceMessage::CallServiceInitialized(result) => match result {
            Ok(service) => {
                let local_peer_id = service.local_id().to_string();
                let signaling_port = service.signaling_port();
                let persist_local_peer_id = app.ctx.config.identity.peer_id.is_none()
                    && std::env::var_os("FJARSYN_PEER_ID").is_none();
                app.runtime.services.call_service = Some(service);
                ServiceAction::CallServiceReady {
                    local_peer_id,
                    signaling_port,
                    persist_local_peer_id,
                }
            }
            Err(err) => ServiceAction::CallServiceInitFailed(err.to_string()),
        },
        CallServiceMessage::DiscoveryServiceInitialized(result) => match result {
            Ok(service) => {
                app.runtime.services.discovery_service = Some(service);
                ServiceAction::DiscoveryServiceReady
            }
            Err(err) => ServiceAction::DiscoveryServiceInitFailed(err.to_string()),
        },
        CallServiceMessage::CallEvent(event) => ServiceAction::CallEvent(event),
        CallServiceMessage::DiscoveryEvent(event) => ServiceAction::DiscoveryEvent(event),
        CallServiceMessage::PeerFound(peer) => ServiceAction::PeerFound(peer),
    };

    app_event::execute_app_event(app, AppEvent::Service(action))
}

pub fn handle_capture_msg(app: &mut Fjarsyn, message: CaptureMessage) -> Task<Message> {
    match message {
        CaptureMessage::CaptureInitialized(result) => {
            app.ctx.media.capture_initializing = false;

            match result {
                Ok(provider) => {
                    let retry_start_capture =
                        if let ActiveScreen::Call(screen) = &mut app.active_screen {
                            screen.set_capture_provider(provider.clone())
                        } else {
                            false
                        };

                    app.ctx.media.capture = Some(provider);
                    tracing::info!("Capture ready.");

                    if retry_start_capture {
                        Task::done(Message::Screen(ScreenMessage::Call(
                            crate::ui::screens::call::CallMessage::StartCapture,
                        )))
                    } else {
                        Task::none()
                    }
                }
                Err(err) => {
                    if let ActiveScreen::Call(screen) = &mut app.active_screen {
                        screen.mark_capture_init_failed();
                    }
                    app.ctx.notify_error(format!("Capture Failed: {}", err));
                    Task::none()
                }
            }
        }
    }
}

pub fn handle_database_msg(app: &mut Fjarsyn, message: DatabaseMessage) -> Task<Message> {
    let action = match message {
        DatabaseMessage::DatabaseInitialized(result) => match result {
            Ok(pool) => {
                app.runtime.db = Some(pool.clone());
                app.runtime.services.contacts_service =
                    Some(Arc::new(ContactsService::new(Arc::new(ContactsRepository::new(pool)))));
                ServiceAction::DatabaseReady
            }
            Err(err) => ServiceAction::DatabaseInitFailed(err.to_string()),
        },
    };

    app_event::execute_app_event(app, AppEvent::Service(action))
}

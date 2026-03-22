use std::sync::Arc;

use sqlx::SqlitePool;

use crate::{
    networking::discovery::{DiscoveryEvent, PeerInfo},
    services::{
        call_service::{CallEvent, CallService},
        contacts_service::ContactsService,
        messaging_service::MessagingEvent,
    },
    ui::{
        app::{ActiveScreen, Fjarsyn},
        message::{CallServiceMessage, CaptureMessage, DatabaseMessage, Route},
    },
};

pub(crate) enum ServiceEffect {
    NotifyError(String),
    NotifyInfo(String),
    SaveConfig,
    Navigate(Route),
    LoadContacts,
    InitializeMessaging {
        db: SqlitePool,
        webrtc: Arc<crate::networking::webrtc::WebRTC>,
        event_tx: tokio::sync::mpsc::Sender<MessagingEvent>,
    },
    RetryCallCaptureStart,
}

// Service workflows update application state and emit follow-up work. The
// handler layer only interprets these effects into Iced tasks.
pub(crate) fn reduce_call_service(
    app: &mut Fjarsyn,
    message: CallServiceMessage,
) -> Vec<ServiceEffect> {
    match message {
        CallServiceMessage::CallServiceInitialized(result) => match result {
            Ok(service) => reduce_call_service_initialized(app, service),
            Err(err) => {
                vec![ServiceEffect::NotifyError(format!(
                    "Call service failed to initialize: {}",
                    err
                ))]
            }
        },
        CallServiceMessage::CallEvent(event) => reduce_call_event(app, event),
        CallServiceMessage::DiscoveryEvent(event) => {
            reduce_discovery_event(app, event);
            Vec::new()
        }
        CallServiceMessage::PeerFound(peer) => {
            maybe_upsert_peer(app, peer);
            Vec::new()
        }
        CallServiceMessage::PeerRemoved(id) => {
            app.ctx.networking.discovered_peers.retain(|peer| peer.id != id);
            Vec::new()
        }
    }
}

pub(crate) fn reduce_capture(app: &mut Fjarsyn, message: CaptureMessage) -> Vec<ServiceEffect> {
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
                        vec![ServiceEffect::RetryCallCaptureStart]
                    } else {
                        Vec::new()
                    }
                }
                Err(err) => {
                    if let ActiveScreen::Call(screen) = &mut app.active_screen {
                        screen.mark_capture_init_failed();
                    }
                    vec![ServiceEffect::NotifyError(format!("Capture Failed: {}", err))]
                }
            }
        }
    }
}

pub(crate) fn reduce_database(app: &mut Fjarsyn, message: DatabaseMessage) -> Vec<ServiceEffect> {
    match message {
        DatabaseMessage::DatabaseInitialized(result) => match result {
            Ok(pool) => reduce_database_initialized(app, pool),
            Err(err) => vec![ServiceEffect::NotifyError(format!("DB Failed: {}", err))],
        },
    }
}

fn reduce_call_service_initialized(
    app: &mut Fjarsyn,
    service: Arc<CallService>,
) -> Vec<ServiceEffect> {
    let mut effects = Vec::new();

    if app.ctx.config.identity.peer_id.is_none() {
        app.ctx.config.identity.peer_id = Some(service.local_id().to_string());
        effects.push(ServiceEffect::SaveConfig);
    }

    app.ctx.services.call_service = Some(service);
    if let Some(effect) = maybe_initialize_messaging_service(app) {
        effects.push(effect);
    }
    effects
}

fn reduce_call_event(app: &mut Fjarsyn, event: CallEvent) -> Vec<ServiceEffect> {
    match event {
        CallEvent::IncomingCall { peer_id } => {
            app.ctx.session.target_id = Some(peer_id.clone());
            app.ctx.session.target_label = app
                .ctx
                .networking
                .discovered_peers
                .iter()
                .find(|peer| peer.id == peer_id)
                .map(|peer| peer.instance_name.trim().to_string())
                .filter(|name| !name.is_empty());
            app.ctx.session.incoming_call_id = Some(peer_id);
            app.ctx.session.incoming_call_timeout =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
            app.ctx.session.call_connected = false;
            Vec::new()
        }
        CallEvent::CallConnected => {
            app.ctx.session.incoming_call_id = None;
            app.ctx.session.incoming_call_timeout = None;
            app.ctx.session.call_connected = true;
            update_recent_peer(app);

            if matches!(app.active_screen, ActiveScreen::Call(_)) {
                Vec::new()
            } else {
                vec![ServiceEffect::Navigate(Route::Call)]
            }
        }
        CallEvent::CallEnded => {
            let had_target = app.ctx.session.target_id.is_some();
            app.ctx.session.target_id = None;
            app.ctx.session.target_label = None;
            app.ctx.session.incoming_call_id = None;
            app.ctx.session.incoming_call_timeout = None;
            app.ctx.session.call_connected = false;

            if had_target {
                vec![ServiceEffect::NotifyInfo("Call ended.".into())]
            } else {
                Vec::new()
            }
        }
        CallEvent::RemoteStreamStarted | CallEvent::RemoteStreamEnded => Vec::new(),
    }
}

fn reduce_discovery_event(app: &mut Fjarsyn, event: DiscoveryEvent) {
    match event {
        DiscoveryEvent::PeerFound(peer) => maybe_upsert_peer(app, peer),
        DiscoveryEvent::PeerRemoved(fullname) => {
            app.ctx.networking.discovered_peers.retain(|peer| peer.instance_name != fullname);
        }
    }
}

fn reduce_database_initialized(app: &mut Fjarsyn, pool: SqlitePool) -> Vec<ServiceEffect> {
    app.ctx.db = Some(pool.clone());
    app.ctx.services.contacts_service = Some(Arc::new(ContactsService::new(pool)));
    let mut effects = vec![ServiceEffect::LoadContacts];
    if let Some(effect) = maybe_initialize_messaging_service(app) {
        effects.push(effect);
    }
    effects
}

fn maybe_upsert_peer(app: &mut Fjarsyn, peer: PeerInfo) {
    if app
        .ctx
        .services
        .call_service
        .as_ref()
        .map(|service| service.local_id() == peer.id)
        .unwrap_or(false)
    {
        return;
    }

    if let Some(existing) = app.ctx.networking.discovered_peers.iter_mut().find(|p| p.id == peer.id)
    {
        existing.update(peer);
    } else {
        app.ctx.networking.discovered_peers.push(peer);
    }
}

fn update_recent_peer(app: &mut Fjarsyn) {
    if let Some(target_id) = &app.ctx.session.target_id
        && let Some(peer) =
            app.ctx.networking.discovered_peers.iter().find(|peer| peer.id == *target_id).cloned()
    {
        app.ctx.networking.recent_peers.retain(|recent| recent.id != peer.id);
        app.ctx.networking.recent_peers.insert(0, peer);
    }
}

fn maybe_initialize_messaging_service(app: &mut Fjarsyn) -> Option<ServiceEffect> {
    if app.ctx.services.messaging_service.is_some() {
        return None;
    }

    let db = app.ctx.db.clone()?;
    let call_service = app.ctx.services.call_service.clone()?;
    let event_tx = app.ctx.messaging.event_tx.clone()?;

    Some(ServiceEffect::InitializeMessaging { db, webrtc: call_service.webrtc(), event_tx })
}

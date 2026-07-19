mod composition;
mod media;
mod resolvers;

use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, RwLock},
};

pub use composition::start_application_runtime;
use fjarsyn_core::{
    database,
    peer_session::{
        PeerId, PeerSessionEvent, PeerSessionService, PeerSessionServiceHandle,
        PeerSessionServiceSnapshot,
    },
    presence::{PresenceHandle, PresenceService, PresenceSnapshot},
    services::{
        codec_service::Service,
        contact_trust_service::ContactTrustService,
        messaging_service::{
            ConversationMessage, ConversationSummary, MessagingEvent, MessagingService,
            MessagingServiceHandle,
        },
    },
};
pub use media::{
    LocalMediaState, MediaCodecDirection, MediaEvent, MediaProjection, MediaSessionProjection,
    RemoteMediaState, SessionMediaService, ShareMediaBinding,
};
use tokio::{sync::Mutex, task::JoinHandle};

/// Read-only/command handles shared by the native application layer.
pub struct ApplicationHandles {
    pub contacts: Arc<ContactTrustService>,
    pub sessions: PeerSessionServiceHandle,
    pub presence: PresenceHandle,
    pub messaging: MessagingServiceHandle,
    pub media: Arc<Mutex<SessionMediaService>>,
}

pub(crate) struct ApplicationOwners {
    database: sqlx::SqlitePool,
    codecs: Service,
    sessions: PeerSessionService,
    presence: PresenceService,
    messaging: MessagingService,
    event_workers: Vec<JoinHandle<()>>,
}

/// Application-scoped owners. Screens only see projections and never own any
/// of these network, persistence, or media lifetimes.
pub struct ApplicationRuntime {
    pub handles: ApplicationHandles,
    pub local_peer_id: PeerId,
    pub local_public_key: String,
    /// Configuration captured by services at startup. Persisted UI settings
    /// may differ until a restart applies service-level network changes.
    pub active_config: fjarsyn_core::config::Config,
    media_config: Arc<RwLock<fjarsyn_core::config::Config>>,
    database: Option<sqlx::SqlitePool>,
    codecs: Option<Service>,
    sessions: Option<PeerSessionService>,
    presence: Option<PresenceService>,
    messaging: Option<MessagingService>,
    event_workers: Vec<JoinHandle<()>>,
}

impl ApplicationRuntime {
    pub(crate) fn new(
        handles: ApplicationHandles,
        local_peer_id: PeerId,
        local_public_key: String,
        active_config: fjarsyn_core::config::Config,
        media_config: Arc<RwLock<fjarsyn_core::config::Config>>,
        owners: ApplicationOwners,
    ) -> Self {
        let ApplicationOwners { database, codecs, sessions, presence, messaging, event_workers } =
            owners;
        Self {
            handles,
            local_peer_id,
            local_public_key,
            active_config,
            media_config,
            database: Some(database),
            codecs: Some(codecs),
            sessions: Some(sessions),
            presence: Some(presence),
            messaging: Some(messaging),
            event_workers,
        }
    }

    pub async fn shutdown(mut self) -> Result<(), String> {
        let media_deadline = tokio::time::Instant::now() + media::PIPELINE_SHUTDOWN_TIMEOUT;
        let mut errors = Vec::new();
        // Signal codec supervisors before awaiting any application lock. A
        // local share start may currently hold the media mutex while awaiting
        // codec initialization; this makes that await resolve immediately and
        // starts the same three-second worker-stop budget up front.
        if let Some(codecs) = self.codecs.as_ref() {
            codecs.request_shutdown();
        }
        // Stop projection/reconciliation first so no worker can recreate a
        // media pipeline while the authoritative owners are winding down.
        let event_workers = self.event_workers.drain(..).collect::<Vec<_>>();
        for worker in &event_workers {
            worker.abort();
        }
        if tokio::time::timeout_at(media_deadline, futures::future::join_all(event_workers))
            .await
            .is_err()
        {
            errors.push("media event workers exceeded the shared shutdown deadline".into());
        }

        match tokio::time::timeout_at(media_deadline, self.handles.media.lock()).await {
            Ok(mut media) => media.shutdown_until(media_deadline).await,
            Err(_) => {
                errors.push("media service lock exceeded the shared shutdown deadline".into())
            }
        }

        if let Some(codecs) = self.codecs.take()
            && let Err(error) = codecs.shutdown_until(media_deadline).await
        {
            errors.push(format!("codecs: {error}"));
        }
        if let Some(presence) = self.presence.take()
            && let Err(error) = presence.shutdown().await
        {
            errors.push(format!("presence: {error}"));
        }
        if let Some(sessions) = self.sessions.take()
            && let Err(error) = sessions.shutdown().await
        {
            errors.push(format!("peer sessions: {error}"));
        }
        // Messaging remains alive until peer-session shutdown has drained its
        // mandatory event relay, so final authenticated inbound messages and
        // receipts can still reach persistence.
        if let Some(messaging) = self.messaging.take()
            && let Err(error) = messaging.shutdown().await
        {
            errors.push(format!("messaging: {error}"));
        }

        if let Some(database) = self.database.take() {
            database.close().await;
        }

        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    pub fn update_media_config(&self, config: &fjarsyn_core::config::Config) {
        *self.media_config.write().unwrap_or_else(|poisoned| poisoned.into_inner()) =
            config.clone();
    }
}

impl Drop for ApplicationRuntime {
    fn drop(&mut self) {
        // Explicit `shutdown` is the normal path. This guard prevents detached
        // projection/media tasks if startup delivery or window teardown drops
        // the owner before the async shutdown task can run.
        for worker in self.event_workers.drain(..) {
            worker.abort();
        }
        if let Ok(mut media) = self.handles.media.try_lock() {
            media.cancel_now();
        }
        drop(self.codecs.take());
        drop(self.presence.take());
        drop(self.sessions.take());
        drop(self.messaging.take());
        drop(self.database.take());
    }
}

impl fmt::Debug for ApplicationRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationRuntime")
            .field("local_peer_id", &self.local_peer_id)
            .finish_non_exhaustive()
    }
}

/// Cloneable one-shot carrier used by Iced task messages. The UI handler takes
/// the runtime owner exactly once.
#[derive(Clone)]
pub struct RuntimeSlot(Arc<std::sync::Mutex<Option<ApplicationRuntime>>>);

impl RuntimeSlot {
    pub fn new(runtime: ApplicationRuntime) -> Self {
        Self(Arc::new(std::sync::Mutex::new(Some(runtime))))
    }

    pub fn take(&self) -> Option<ApplicationRuntime> {
        self.0.lock().ok()?.take()
    }
}

impl fmt::Debug for RuntimeSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("RuntimeSlot").field(&"<application runtime>").finish()
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Presence(PresenceSnapshot),
    Sessions(PeerSessionServiceSnapshot),
    SessionEvent(PeerSessionEvent),
    Messaging {
        revision: u64,
        summaries: Arc<Vec<ConversationSummary>>,
        conversations: Arc<BTreeMap<PeerId, Arc<Vec<ConversationMessage>>>>,
    },
    MessagingEvent(MessagingEvent),
    Media(MediaEvent),
}

pub(crate) async fn initialize_database() -> Result<sqlx::SqlitePool, String> {
    database::init().await.map_err(|error| error.to_string())
}

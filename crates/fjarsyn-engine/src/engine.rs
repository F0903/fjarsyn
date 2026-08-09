use std::{fmt, sync::Arc, time::Duration};

use sqlx::SqlitePool;
use tokio::{sync::mpsc, time::Instant};

use crate::{
    Services,
    contacts::{self, ContactsService},
    deferred_resolver::DeferredResolver,
    error::{ShutdownError, StartError, StartupStage},
    identity::{LocalIdentity, PeerId, Store as IdentityStore},
    media::codec,
    messaging, peer_session, presence, screen_share,
    service_host::{ServiceHost, ServicePolicy, ShutdownContext},
    settings::Settings,
};

const ENGINE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_ROLLBACK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ShutdownPhase {
    ScreenShare,
    Codecs,
    Presence,
    PeerSessions,
    Messaging,
}

/// Owns the complete headless application graph.
///
/// UI layers retain this aggregate and borrow only its narrow command and
/// snapshot handles. Concrete stores, service owners, and the database pool do
/// not cross the engine boundary.
pub struct Engine {
    active_settings: Settings,
    local_peer_id: PeerId,
    local_public_key: String,
    services: Services,
    hosted_services: ServiceHost<ShutdownPhase>,
    database: SqlitePool,
}

impl Engine {
    pub async fn start(settings: Settings) -> Result<Self, StartError> {
        let settings = settings
            .validated()
            .map_err(|error| StartError::new(StartupStage::Settings, error, None))?;
        let local_identity = load_local_identity().await?;
        let database = crate::database::init()
            .await
            .map_err(|error| StartError::new(StartupStage::Database, error, None))?;
        let mut startup = Startup::new(database);
        let initialized = Self::init_services(&settings, local_identity, &mut startup).await?;
        let Startup { database, host: hosted_services } = startup;

        Ok(Self {
            active_settings: settings,
            local_peer_id: initialized.local_peer_id,
            local_public_key: initialized.local_public_key,
            services: initialized.services,
            hosted_services,
            database,
        })
    }

    async fn init_services(
        settings: &Settings,
        local_identity: LocalIdentity,
        startup: &mut Startup,
    ) -> Result<InitializedServices, StartError> {
        let directory_result = contacts::Directory::load(Arc::new(contacts::SqliteStore::new(
            startup.database.clone(),
        )))
        .await;
        let directory = Arc::new(startup.require(StartupStage::Contacts, directory_result).await?);

        let endpoints = Arc::new(DeferredResolver::default());
        let (session_event_tx, session_event_rx) = mpsc::channel(messaging::SESSION_EVENT_CAPACITY);
        let mut session_config = peer_session::Config::new(
            local_identity,
            directory.clone(),
            endpoints.clone(),
            peer_session::NetworkScope::AllInterfaces,
        );
        session_config.mandatory_event_sink = Some(session_event_tx);
        session_config.max_depacket_latency =
            Duration::from_millis(u64::from(settings.network.max_depacket_latency_ms));

        let sessions_result = peer_session::PeerSessionService::start(session_config).await;
        let sessions = startup.require(StartupStage::PeerSessions, sessions_result).await?;
        let local_peer_id = sessions.local_peer_id().clone();
        let local_public_key = sessions.local_public_key();
        let signaling_port = sessions.signaling_port();
        let session_handle =
            startup.host.install(sessions, ServicePolicy::new(ShutdownPhase::PeerSessions));

        let presence_result = presence::PresenceService::start(presence::Config::new(
            local_peer_id.clone(),
            signaling_port,
        ));
        let presence = startup.require(StartupStage::Presence, presence_result).await?;
        let presence_handle =
            startup.host.install(presence, ServicePolicy::new(ShutdownPhase::Presence));
        endpoints.install(presence_handle.clone());

        let contacts =
            ContactsService::new(directory, session_handle.clone(), local_peer_id.clone());
        let messaging_result = messaging::MessagingService::start(messaging::Config {
            store: Arc::new(messaging::SqliteStore::new(startup.database.clone())),
            sessions: session_handle.clone(),
            session_events: session_event_rx,
            limits: messaging::Limits::default(),
        })
        .await;
        let messaging = startup.require(StartupStage::Messaging, messaging_result).await?;
        let messaging_handle =
            startup.host.install(messaging, ServicePolicy::new(ShutdownPhase::Messaging));

        let codecs = codec::CodecService::start(codec::Config::default());
        let codec_handle =
            startup.host.install(codecs, ServicePolicy::new(ShutdownPhase::Codecs).prepare_early());
        let screen_share = screen_share::ScreenShareService::start(
            screen_share::Config::from(settings),
            session_handle.clone(),
            codec_handle.clone(),
        );
        let screen_share_handle =
            startup.host.install(screen_share, ServicePolicy::new(ShutdownPhase::ScreenShare));

        Ok(InitializedServices {
            local_peer_id,
            local_public_key,
            services: Services::new(
                contacts,
                session_handle,
                presence_handle,
                messaging_handle,
                codec_handle,
                screen_share_handle,
            ),
        })
    }

    /// Configuration captured when the engine graph was started. Persisted UI
    /// settings may differ until a restart applies service-level changes.
    pub fn active_settings(&self) -> &Settings {
        &self.active_settings
    }

    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    pub fn local_public_key(&self) -> &str {
        &self.local_public_key
    }

    pub fn services(&self) -> &Services {
        &self.services
    }

    /// Gracefully stops the complete engine within one absolute three-second
    /// deadline, then cancels or detaches work that has not finished.
    pub async fn shutdown(self) -> Result<(), ShutdownError> {
        let deadline = Instant::now() + ENGINE_SHUTDOWN_TIMEOUT;
        let Self {
            active_settings,
            local_peer_id,
            local_public_key,
            services,
            mut hosted_services,
            database,
        } = self;
        drop((active_settings, local_peer_id, local_public_key, services));

        match shutdown_resources(&mut hosted_services, &database, deadline).await {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Engine")
            .field("local_peer_id", &self.local_peer_id)
            .field("services", &self.services)
            .field("hosted_services", &self.hosted_services)
            .finish_non_exhaustive()
    }
}

async fn load_local_identity() -> Result<LocalIdentity, StartError> {
    let loaded = tokio::task::spawn_blocking(|| IdentityStore::user().load_or_create())
        .await
        .map_err(|error| StartError::new(StartupStage::Identity, error, None))?;
    loaded.map_err(|error| StartError::new(StartupStage::Identity, error, None))
}

struct InitializedServices {
    local_peer_id: PeerId,
    local_public_key: String,
    services: Services,
}

struct Startup {
    host: ServiceHost<ShutdownPhase>,
    database: SqlitePool,
}

impl Startup {
    fn new(database: SqlitePool) -> Self {
        Self { host: ServiceHost::new(), database }
    }

    async fn require<Value, Error>(
        &mut self,
        stage: StartupStage,
        result: Result<Value, Error>,
    ) -> Result<Value, StartError>
    where
        Error: std::error::Error + Send + Sync + 'static,
    {
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(self.fail(stage, error).await),
        }
    }

    async fn fail(
        &mut self,
        stage: StartupStage,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> StartError {
        let deadline = Instant::now() + STARTUP_ROLLBACK_TIMEOUT;
        let rollback = shutdown_resources(&mut self.host, &self.database, deadline).await;
        StartError::new(stage, source, rollback)
    }
}

async fn shutdown_resources(
    hosted_services: &mut ServiceHost<ShutdownPhase>,
    database: &SqlitePool,
    deadline: Instant,
) -> Option<ShutdownError> {
    let context = ShutdownContext::new(Some(deadline));
    // Codec preparation must happen synchronously before screen-share
    // shutdown is awaited: a pipeline may be blocked on codec startup.
    hosted_services.prepare_shutdown(context);
    let failures = hosted_services.shutdown(context).await;

    // Constructing SQLx's close future marks the pool closed immediately;
    // awaiting it only drains outstanding connections. If the shared
    // deadline is exhausted, dropping the future leaves final connection
    // disposal to the pool's ordinary drop path.
    let close = database.close();
    let database_close_timed_out = if Instant::now() >= deadline {
        drop(close);
        true
    } else {
        tokio::time::timeout_at(deadline, close).await.is_err()
    };
    ShutdownError::from_shutdown(failures, database_close_timed_out)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    #[tokio::test]
    async fn invalid_settings_fail_at_the_engine_boundary() {
        let mut settings = Settings::default();
        settings.video.target_bitrate_bps = 0;

        let error = Engine::start(settings).await.unwrap_err();

        assert_eq!(error.stage(), StartupStage::Settings);
    }

    #[tokio::test]
    async fn shared_deadline_bounds_database_drain_and_closes_admission() {
        let database =
            SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        let connection = database.acquire().await.unwrap();
        let mut hosted_services = ServiceHost::new();

        let error = shutdown_resources(
            &mut hosted_services,
            &database,
            Instant::now() + Duration::from_millis(10),
        )
        .await
        .expect("the held connection should exhaust the drain deadline");

        assert!(error.failures().is_empty());
        assert!(error.database_close_timed_out());
        assert!(database.is_closed());
        drop(connection);
    }
}

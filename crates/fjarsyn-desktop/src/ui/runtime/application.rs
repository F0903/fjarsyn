use std::{
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use fjarsyn_engine::{
    Engine, config::Config, contacts, identity::PeerId, messaging, peer_session, presence,
    screen_share,
};
use tokio::sync::mpsc;

use crate::ui::runtime::{Event, projection};

const PROJECTION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

/// Owns the headless engine together with desktop-only runtime projections.
pub(in crate::ui) struct Application {
    projections: projection::Workers,
    engine: Option<Engine>,
}

impl Application {
    pub(in crate::ui) async fn start(
        config: Config,
        event_tx: mpsc::Sender<Event>,
    ) -> Result<Self, String> {
        let engine = Engine::start(config).await.map_err(|error| error.to_string())?;
        let projections = projection::Workers::start(
            engine.services().presence(),
            engine.services().sessions(),
            engine.services().messaging(),
            engine.services().screen_share(),
            event_tx,
        );

        Ok(Self { projections, engine: Some(engine) })
    }

    pub(in crate::ui) async fn shutdown(mut self) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + PROJECTION_SHUTDOWN_TIMEOUT;
        let mut errors = Vec::new();

        if !self.projections.shutdown_until(deadline).await {
            errors.push("projection workers exceeded their shutdown deadline".into());
        }
        if let Some(engine) = self.engine.take()
            && let Err(error) = engine.shutdown().await
        {
            errors.push(error.to_string());
        }

        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    pub(in crate::ui) fn active_config(&self) -> &Config {
        self.engine().active_config()
    }

    pub(in crate::ui) fn local_peer_id(&self) -> &PeerId {
        self.engine().local_peer_id()
    }

    pub(in crate::ui) fn local_public_key(&self) -> &str {
        self.engine().local_public_key()
    }

    pub(in crate::ui) fn contacts(&self) -> &contacts::ContactsService {
        self.engine().services().contacts()
    }

    pub(in crate::ui) fn sessions(&self) -> &peer_session::ServiceHandle {
        self.engine().services().sessions()
    }

    pub(in crate::ui) fn presence(&self) -> &presence::ServiceHandle {
        self.engine().services().presence()
    }

    pub(in crate::ui) fn messaging(&self) -> &messaging::ServiceHandle {
        self.engine().services().messaging()
    }

    pub(in crate::ui) fn screen_share(&self) -> &screen_share::ServiceHandle {
        self.engine().services().screen_share()
    }

    fn engine(&self) -> &Engine {
        self.engine.as_ref().expect("desktop engine owner is available before shutdown")
    }

    fn abort(&mut self) {
        self.projections.abort();
        drop(self.engine.take());
    }
}

impl fmt::Debug for Application {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Application")
            .field("local_peer_id", &self.local_peer_id())
            .finish_non_exhaustive()
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        self.abort();
    }
}

/// Cloneable one-shot carrier used by Iced task messages. The UI handler takes
/// the application owner exactly once.
#[derive(Clone)]
pub(in crate::ui) struct Slot(Arc<Mutex<Option<Application>>>);

impl Slot {
    pub(in crate::ui) fn new(application: Application) -> Self {
        Self(Arc::new(Mutex::new(Some(application))))
    }

    pub(in crate::ui) fn take(&self) -> Option<Application> {
        self.0.lock().ok()?.take()
    }
}

impl fmt::Debug for Slot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("Slot").field(&"<application>").finish()
    }
}

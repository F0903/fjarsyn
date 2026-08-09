use std::{
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use fjarsyn_engine::{
    Engine, contacts, identity::PeerId, messaging, peer_session, screen_share, settings,
};

use crate::ui::runtime::{RuntimeId, engine_adapter};

const ENGINE_ADAPTER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

/// Owns the headless engine and its complete desktop adaptation boundary.
pub(in crate::ui) struct EngineRuntime {
    engine_adapter: engine_adapter::EngineAdapter,
    receivers: engine_adapter::Receivers,
    engine: Option<Engine>,
}

impl EngineRuntime {
    pub(in crate::ui) async fn start(
        runtime_id: RuntimeId,
        settings: settings::Settings,
    ) -> Result<Self, String> {
        let engine = Engine::start(settings).await.map_err(|error| error.to_string())?;
        let (engine_adapter, receivers) = engine_adapter::EngineAdapter::start(
            runtime_id,
            engine.services().presence(),
            engine.services().sessions(),
            engine.services().messaging(),
            engine.services().screen_share(),
        );

        Ok(Self { engine_adapter, receivers, engine: Some(engine) })
    }

    pub(in crate::ui) async fn shutdown(mut self) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + ENGINE_ADAPTER_SHUTDOWN_TIMEOUT;
        let mut errors = Vec::new();

        match self.engine_adapter.shutdown_until(deadline).await {
            engine_adapter::Shutdown::Clean => {}
            engine_adapter::Shutdown::Failed(failure) => errors.push(failure.to_string()),
            engine_adapter::Shutdown::TimedOut => {
                errors.push("engine adapter exceeded its shutdown deadline".into());
            }
        }
        if let Some(engine) = self.engine.take()
            && let Err(error) = engine.shutdown().await
        {
            errors.push(error.to_string());
        }

        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    pub(in crate::ui) fn active_settings(&self) -> &settings::Settings {
        self.engine().active_settings()
    }

    pub(in crate::ui) const fn runtime_id(&self) -> RuntimeId {
        self.receivers.runtime_id
    }

    pub(in crate::ui) fn receivers(&self) -> engine_adapter::Receivers {
        self.receivers.clone()
    }

    pub(in crate::ui) fn latest_state(&mut self) -> engine_adapter::EngineState {
        self.receivers.state.latest()
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
        self.engine_adapter.abort();
        drop(self.engine.take());
    }
}

impl fmt::Debug for EngineRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineRuntime")
            .field("runtime_id", &self.runtime_id())
            .field("local_peer_id", &self.local_peer_id())
            .finish_non_exhaustive()
    }
}

impl Drop for EngineRuntime {
    fn drop(&mut self) {
        self.abort();
    }
}

/// Cloneable one-shot carrier used by Iced task messages. The UI handler takes
/// the engine-runtime owner exactly once.
#[derive(Clone)]
pub(in crate::ui) struct RuntimeSlot(Arc<Mutex<Option<EngineRuntime>>>);

impl RuntimeSlot {
    pub(in crate::ui) fn new(runtime: EngineRuntime) -> Self {
        Self(Arc::new(Mutex::new(Some(runtime))))
    }

    pub(in crate::ui) fn take(&self) -> Option<EngineRuntime> {
        self.0.lock().ok()?.take()
    }
}

impl fmt::Debug for RuntimeSlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("RuntimeSlot").field(&"<engine-runtime>").finish()
    }
}

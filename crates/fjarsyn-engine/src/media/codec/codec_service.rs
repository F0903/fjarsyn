//! Application-owned codec service lifecycle.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;

use crate::{
    media::codec::{
        REAPER_POLL_INTERVAL, ServiceHandle, State,
        backend::{BackendFactory, FfmpegBackendFactory},
    },
    service_host::{HostedService, ShutdownContext},
};

#[derive(Debug, Clone)]
pub struct Config {
    pub call_timeout: Duration,
    pub stop_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self { call_timeout: Duration::from_secs(10), stop_timeout: Duration::from_secs(3) }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{remaining_workers} codec worker(s) did not stop before the shutdown deadline")]
pub struct ShutdownError {
    pub remaining_workers: usize,
}

pub struct CodecService {
    state: Arc<State>,
}

impl CodecService {
    pub fn start(config: Config) -> Self {
        Self::start_with_backend(config, Arc::new(FfmpegBackendFactory))
    }

    pub(in crate::media::codec) fn start_with_backend(
        config: Config,
        backend: Arc<dyn BackendFactory>,
    ) -> Self {
        let state = State::new(config, backend);
        Self { state }
    }

    /// Synchronously prevents new workers and asks every current worker to
    /// stop. Call this before awaiting owners that may themselves be blocked on
    /// codec startup; hosted shutdown performs the bounded join afterward.
    pub(crate) fn request_shutdown(&self) {
        self.state.begin_shutdown();
    }

    #[cfg(test)]
    pub(crate) async fn shutdown(mut self) -> Result<(), ShutdownError> {
        let deadline = tokio::time::Instant::now() + self.state.stop_timeout();
        self.shutdown_until(deadline).await
    }

    /// Completes service shutdown within a caller-owned absolute
    /// deadline. The caller may pre-signal with [`Self::request_shutdown`] so
    /// media and codec cleanup consume the same time budget.
    pub(crate) async fn shutdown_until(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), ShutdownError> {
        self.state.begin_shutdown();
        loop {
            let supervisor_tasks = self.state.try_observe_supervisor_tasks();
            let (active, quarantined) = self.state.shutdown_counts();
            if active == 0 && supervisor_tasks == Some(0) {
                return if quarantined == 0 {
                    Ok(())
                } else {
                    Err(ShutdownError { remaining_workers: quarantined })
                };
            }
            if tokio::time::Instant::now() >= deadline {
                self.state.poison_unfinished_shutdowns();
                // Never abort a supervisor at the service deadline: it owns
                // the native JoinHandle and must remain alive long enough to
                // quarantine and reap an in-flight native call. Detaching the
                // Tokio handle is an explicit bounded-shutdown handoff, not
                // cancellation of the supervisor future.
                let detached_supervisors = self.state.try_detach_supervisor_tasks();
                let (active, quarantined) = self.state.shutdown_counts();
                self.state.detach_unfinished_workers();
                let remaining_workers =
                    active.saturating_add(quarantined).max(detached_supervisors.unwrap_or(1));
                return Err(ShutdownError { remaining_workers });
            }
            let next_poll = tokio::time::Instant::now() + REAPER_POLL_INTERVAL;
            tokio::time::sleep_until(std::cmp::min(next_poll, deadline)).await;
        }
    }
}

#[async_trait]
impl HostedService for CodecService {
    const NAME: &'static str = "codecs";

    type ServiceHandle = ServiceHandle;
    type Error = ShutdownError;

    fn service_handle(&self) -> Self::ServiceHandle {
        ServiceHandle::new(self.state.clone())
    }

    fn prepare_shutdown(&mut self, _context: ShutdownContext) {
        self.request_shutdown();
    }

    async fn shutdown(&mut self, context: ShutdownContext) -> Result<(), Self::Error> {
        let deadline = context.bounded_deadline(self.state.stop_timeout());
        self.shutdown_until(deadline).await
    }

    fn cancel(&mut self) {
        // Native calls cannot be forcefully interrupted safely. Beginning
        // shutdown is the strongest truthful synchronous cancellation this
        // boundary can provide; supervisors quarantine calls that outlive it.
        self.request_shutdown();
    }
}

impl Drop for CodecService {
    fn drop(&mut self) {
        self.state.begin_shutdown();
    }
}

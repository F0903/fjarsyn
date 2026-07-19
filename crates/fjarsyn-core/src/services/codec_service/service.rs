//! Application-owned codec service lifecycle.

use std::{sync::Arc, time::Duration};

use crate::services::codec_service::{
    Config, Handle, ShutdownError, State,
    backend::{CodecBackendFactory, FfmpegCodecBackendFactory},
};

const REAPER_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub struct Service {
    state: Arc<State>,
}

impl Service {
    pub fn start(config: Config) -> (Self, Handle) {
        Self::start_with_backend(config, Arc::new(FfmpegCodecBackendFactory))
    }

    pub(in crate::services::codec_service) fn start_with_backend(
        config: Config,
        backend: Arc<dyn CodecBackendFactory>,
    ) -> (Self, Handle) {
        let state = State::new(config, backend);
        (Self { state: state.clone() }, Handle::new(state))
    }

    pub fn handle(&self) -> Handle {
        Handle::new(self.state.clone())
    }

    /// Synchronously prevents new workers and asks every current worker to
    /// stop. Call this before awaiting owners that may themselves be blocked on
    /// codec startup; [`Self::shutdown`] performs the bounded join afterward.
    pub fn request_shutdown(&self) {
        self.state.begin_shutdown();
    }

    pub async fn shutdown(self) -> Result<(), ShutdownError> {
        let deadline = tokio::time::Instant::now() + self.state.stop_timeout();
        self.shutdown_until(deadline).await
    }

    /// Completes service shutdown within an application-owned absolute
    /// deadline. The caller may pre-signal with [`Self::request_shutdown`] so
    /// media and codec cleanup consume the same time budget.
    pub async fn shutdown_until(self, deadline: tokio::time::Instant) -> Result<(), ShutdownError> {
        self.state.begin_shutdown();
        loop {
            let (active, quarantined) = self.state.shutdown_counts();
            if active == 0 {
                return if quarantined == 0 {
                    Ok(())
                } else {
                    Err(ShutdownError { remaining_workers: quarantined })
                };
            }
            if tokio::time::Instant::now() >= deadline {
                self.state.poison_unfinished_shutdowns();
                // Timed-out native threads cannot be joined safely in-process.
                // Their supervisors retain the JoinHandles and quarantine
                // them; lifecycle accounting must not extend application
                // shutdown beyond this one absolute stop deadline.
                self.state.detach_unfinished_workers();
                return Err(ShutdownError { remaining_workers: active + quarantined });
            }
            let next_poll = tokio::time::Instant::now() + REAPER_POLL_INTERVAL;
            tokio::time::sleep_until(std::cmp::min(next_poll, deadline)).await;
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.state.begin_shutdown();
    }
}

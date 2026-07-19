//! Cloneable application-facing codec service handle.

use std::{fmt, sync::Arc};

use tokio::sync::watch;

use crate::services::codec_service::{
    DecoderSession, DecoderWorkerConfig, EncoderSession, EncoderWorkerConfig, Error, Snapshot,
    State, decoder::DecoderSupervisor, encoder::EncoderSupervisor,
};

#[derive(Clone)]
pub struct Handle {
    state: Arc<State>,
}

impl Handle {
    pub(in crate::services::codec_service) fn new(state: Arc<State>) -> Self {
        Self { state }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.snapshot()
    }

    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.state.subscribe()
    }

    pub async fn open_encoder(&self, config: EncoderWorkerConfig) -> Result<EncoderSession, Error> {
        EncoderSupervisor::start(self.state.clone(), config).await
    }

    pub async fn open_decoder(&self, config: DecoderWorkerConfig) -> Result<DecoderSession, Error> {
        DecoderSupervisor::start(self.state.clone(), config).await
    }

    #[cfg(test)]
    pub(in crate::services::codec_service) fn worker_count_for_test(&self) -> usize {
        let (active, quarantined) = self.state.shutdown_counts();
        active + quarantined
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Handle").field("snapshot", &self.snapshot()).finish()
    }
}

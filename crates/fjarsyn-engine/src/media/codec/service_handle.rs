//! Cloneable application-facing codec service handle.

use std::{fmt, sync::Arc};

use tokio::sync::watch;

use crate::media::codec::{
    DecoderSession, DecoderWorkerConfig, EncoderSession, EncoderWorkerConfig, Error, Health, State,
    decoder, encoder,
};

#[derive(Clone)]
pub struct ServiceHandle {
    state: Arc<State>,
}

impl ServiceHandle {
    pub(in crate::media::codec) fn new(state: Arc<State>) -> Self {
        Self { state }
    }

    pub fn snapshot(&self) -> Health {
        self.state.snapshot()
    }

    pub fn subscribe(&self) -> watch::Receiver<Health> {
        self.state.subscribe()
    }

    pub async fn open_encoder(&self, config: EncoderWorkerConfig) -> Result<EncoderSession, Error> {
        encoder::Supervisor::start(self.state.clone(), config).await
    }

    pub async fn open_decoder(&self, config: DecoderWorkerConfig) -> Result<DecoderSession, Error> {
        decoder::Supervisor::start(self.state.clone(), config).await
    }

    #[cfg(test)]
    pub(in crate::media::codec) fn worker_count_for_test(&self) -> usize {
        let (active, quarantined) = self.state.shutdown_counts();
        active + quarantined
    }
}

impl fmt::Debug for ServiceHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ServiceHandle").field("snapshot", &self.snapshot()).finish()
    }
}

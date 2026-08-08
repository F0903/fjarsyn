//! Application-facing handle for one supervised codec worker.

use std::{
    fmt,
    sync::{Arc, Weak},
};

use tokio::sync::watch;

use crate::media::codec::{Poison, State, registry::WorkerId};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum WorkerError {
    #[error("codec failed: {0}")]
    Codec(String),
    #[error("{0}")]
    RestartRequired(Poison),
    #[error("codec service stopped")]
    ServiceStopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::media::codec) enum WorkerCompletion {
    Running,
    Finished(Result<(), WorkerError>),
}

pub struct Worker {
    id: WorkerId,
    state: Weak<State>,
    completion: watch::Receiver<WorkerCompletion>,
}

impl Worker {
    pub(in crate::media::codec) fn new(
        id: WorkerId,
        state: &Arc<State>,
        completion: watch::Receiver<WorkerCompletion>,
    ) -> Self {
        Self { id, state: Arc::downgrade(state), completion }
    }

    pub fn is_finished(&self) -> bool {
        matches!(*self.completion.borrow(), WorkerCompletion::Finished(_))
    }

    pub fn request_stop(&self) {
        if let Some(state) = self.state.upgrade() {
            state.request_stop(self.id);
        }
    }

    pub async fn wait(mut self) -> Result<(), WorkerError> {
        Self::wait_for_completion(&mut self.completion).await
    }

    pub async fn shutdown(mut self) -> Result<(), WorkerError> {
        self.request_stop();
        Self::wait_for_completion(&mut self.completion).await
    }

    async fn wait_for_completion(
        completion: &mut watch::Receiver<WorkerCompletion>,
    ) -> Result<(), WorkerError> {
        loop {
            if let WorkerCompletion::Finished(result) = completion.borrow().clone() {
                return result;
            }
            if completion.changed().await.is_err() {
                return Err(WorkerError::ServiceStopped);
            }
        }
    }
}

impl fmt::Debug for Worker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Worker")
            .field("id", &self.id)
            .field("finished", &self.is_finished())
            .finish()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.request_stop();
    }
}

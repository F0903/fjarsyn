//! Application-facing handle for one supervised codec worker.

use std::{
    fmt,
    sync::{Arc, Weak},
};

use tokio::sync::watch;

use super::WorkerCompletion;
use crate::services::codec_service::{CodecWorkerError, State, registry::WorkerId};

pub struct CodecWorker {
    id: WorkerId,
    state: Weak<State>,
    completion: watch::Receiver<WorkerCompletion>,
}

impl CodecWorker {
    pub(in crate::services::codec_service) fn new(
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

    pub async fn wait(mut self) -> Result<(), CodecWorkerError> {
        Self::wait_for_completion(&mut self.completion).await
    }

    pub async fn shutdown(mut self) -> Result<(), CodecWorkerError> {
        self.request_stop();
        Self::wait_for_completion(&mut self.completion).await
    }

    async fn wait_for_completion(
        completion: &mut watch::Receiver<WorkerCompletion>,
    ) -> Result<(), CodecWorkerError> {
        loop {
            if let WorkerCompletion::Finished(result) = completion.borrow().clone() {
                return result;
            }
            if completion.changed().await.is_err() {
                return Err(CodecWorkerError::ServiceStopped);
            }
        }
    }
}

impl fmt::Debug for CodecWorker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodecWorker")
            .field("id", &self.id)
            .field("finished", &self.is_finished())
            .finish()
    }
}

impl Drop for CodecWorker {
    fn drop(&mut self) {
        self.request_stop();
    }
}

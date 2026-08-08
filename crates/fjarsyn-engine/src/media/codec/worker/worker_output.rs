//! Shared completion-aware receiver embedded by encoder and decoder outputs.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::{mpsc, watch};

use super::WorkerCompletion;
use crate::media::codec::WorkerError;

pub(in crate::media::codec) struct WorkerOutput<T> {
    receiver: mpsc::Receiver<T>,
    completion: watch::Receiver<WorkerCompletion>,
    publishing: Arc<AtomicBool>,
    terminal_emitted: bool,
}

impl<T> WorkerOutput<T> {
    pub(in crate::media::codec) fn new(
        receiver: mpsc::Receiver<T>,
        completion: watch::Receiver<WorkerCompletion>,
        publishing: Arc<AtomicBool>,
    ) -> Self {
        Self { receiver, completion, publishing, terminal_emitted: false }
    }

    pub(in crate::media::codec) async fn recv(&mut self) -> Option<Result<T, WorkerError>> {
        loop {
            if let WorkerCompletion::Finished(result) = self.completion.borrow().clone() {
                if self.terminal_emitted {
                    return None;
                }
                self.terminal_emitted = true;
                self.receiver.close();
                while self.receiver.try_recv().is_ok() {}
                return result.err().map(Err);
            }

            tokio::select! {
                biased;
                changed = self.completion.changed() => {
                    if changed.is_err() {
                        self.terminal_emitted = true;
                        return Some(Err(WorkerError::ServiceStopped));
                    }
                }
                value = self.receiver.recv() => match value {
                    // This acquire load is the publication linearization point.
                    // Direction poison performs the matching release store before
                    // it wakes consumers, so results completed afterward cannot
                    // cross the service boundary.
                    Some(value) if self.publishing.load(Ordering::Acquire) => {
                        return Some(Ok(value));
                    }
                    Some(_) => continue,
                    None => {
                        if self.completion.changed().await.is_err() {
                            self.terminal_emitted = true;
                            return Some(Err(WorkerError::ServiceStopped));
                        }
                    }
                }
            }
        }
    }
}

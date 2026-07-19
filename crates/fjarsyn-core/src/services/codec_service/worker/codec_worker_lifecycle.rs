//! Native thread ownership, bounded finalization, and quarantine reaping.

use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use tokio::sync::watch;

use super::WorkerCompletion;
use crate::services::codec_service::{
    CodecDirection, CodecOperation, CodecPoisonReason, CodecWorkerError, State, registry::WorkerId,
};

const REAPER_POLL_INTERVAL: Duration = Duration::from_millis(25);

type CodecWorkerThreadResult = Result<(), String>;

pub(in crate::services::codec_service) struct CodecWorkerLifecycle {
    state: Arc<State>,
    id: WorkerId,
    direction: CodecDirection,
    thread: JoinHandle<CodecWorkerThreadResult>,
    completion: watch::Sender<WorkerCompletion>,
    accepting: Arc<AtomicBool>,
    publishing: Arc<AtomicBool>,
}

impl CodecWorkerLifecycle {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::services::codec_service) fn spawn<Operation>(
        state: Arc<State>,
        id: WorkerId,
        direction: CodecDirection,
        thread_name: String,
        completion: watch::Sender<WorkerCompletion>,
        accepting: Arc<AtomicBool>,
        publishing: Arc<AtomicBool>,
        operation: Operation,
    ) -> std::io::Result<Self>
    where
        Operation: FnOnce() -> CodecWorkerThreadResult + Send + 'static,
    {
        let thread = std::thread::Builder::new().name(thread_name).spawn(operation)?;
        Ok(Self { state, id, direction, thread, completion, accepting, publishing })
    }

    pub(in crate::services::codec_service) fn state(&self) -> &Arc<State> {
        &self.state
    }

    pub(in crate::services::codec_service) async fn wait_until_thread_finished(&self) {
        while !self.thread.is_finished() {
            tokio::time::sleep(REAPER_POLL_INTERVAL).await;
        }
    }

    pub(in crate::services::codec_service) async fn finish(
        self,
        mut result: Result<(), CodecWorkerError>,
        wait_for_thread: bool,
    ) {
        self.accepting.store(false, Ordering::Release);
        self.publishing.store(false, Ordering::Release);

        let joined = if wait_for_thread {
            match tokio::time::timeout(self.state.stop_timeout(), self.wait_until_thread_finished())
                .await
            {
                Ok(()) => true,
                Err(_) => {
                    let poison = self.state.poison(
                        self.direction,
                        CodecOperation::Shutdown,
                        CodecPoisonReason::DeadlineExceeded,
                    );
                    result = Err(CodecWorkerError::RestartRequired(poison));
                    false
                }
            }
        } else {
            self.thread.is_finished()
        };

        if joined {
            match self.thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) if result.is_ok() => {
                    result = Err(CodecWorkerError::Codec(error));
                }
                Ok(Err(_)) => {}
                Err(_) => {
                    // With the release panic=abort profile this path is only
                    // reachable in unwind-enabled development/test builds.
                    let poison = self.state.poison(
                        self.direction,
                        CodecOperation::Shutdown,
                        CodecPoisonReason::WorkerTerminated,
                    );
                    result = Err(CodecWorkerError::RestartRequired(poison));
                }
            }
            self.completion.send_replace(WorkerCompletion::Finished(result));
            self.state.remove_worker(self.id);
        } else {
            self.completion.send_replace(WorkerCompletion::Finished(result));
            // The OS thread is no longer a usable service worker. Move it out
            // of active accounting so shutdown can report it without waiting
            // through another stop deadline. The detached reaper owns the
            // JoinHandle until the native call returns.
            let tracked = self.state.quarantine_worker(self.id);
            let state = tracked.then(|| Arc::downgrade(&self.state));
            Self::spawn_quarantine_reaper(self.thread, state);
        }
    }

    fn spawn_quarantine_reaper(
        thread: JoinHandle<CodecWorkerThreadResult>,
        state: Option<Weak<State>>,
    ) {
        tokio::spawn(async move {
            while !thread.is_finished() {
                tokio::time::sleep(REAPER_POLL_INTERVAL).await;
            }
            let _ = thread.join();
            if let Some(state) = state.and_then(|state| state.upgrade()) {
                state.release_quarantined_worker();
            }
        });
    }
}

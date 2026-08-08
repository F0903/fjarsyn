use std::time::Duration;

use tokio::{
    sync::watch,
    task::{AbortHandle, JoinHandle},
};

use super::PIPELINE_SHUTDOWN_TIMEOUT;

pub(super) struct ChildTaskGuard {
    aborts: Vec<AbortHandle>,
    armed: bool,
}

impl ChildTaskGuard {
    pub(super) fn new(aborts: Vec<AbortHandle>) -> Self {
        Self { aborts, armed: true }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ChildTaskGuard {
    fn drop(&mut self) {
        if self.armed {
            for abort in &self.aborts {
                abort.abort();
            }
        }
    }
}

pub(super) fn task_failure(
    context: &str,
    result: Result<Result<(), String>, tokio::task::JoinError>,
) -> Option<String> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(error) if error.is_cancelled() => None,
        Err(error) => Some(format!("{context} worker failed: {error}")),
    }
}

pub(super) struct OwnedPipeline {
    pub(super) stop: Option<watch::Sender<bool>>,
    pub(super) task: Option<JoinHandle<()>>,
    pub(super) children: Vec<AbortHandle>,
}

impl OwnedPipeline {
    pub(super) fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub(super) async fn shutdown(mut self) -> bool {
        self.shutdown_with_timeout(PIPELINE_SHUTDOWN_TIMEOUT).await
    }

    pub(super) fn request_stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(true);
        }
    }

    pub(super) async fn shutdown_with_timeout(&mut self, timeout: Duration) -> bool {
        self.shutdown_until(tokio::time::Instant::now() + timeout).await
    }

    pub(super) async fn shutdown_until(&mut self, deadline: tokio::time::Instant) -> bool {
        self.request_stop();
        let Some(task) = self.task.as_mut() else {
            return true;
        };
        let clean = match tokio::time::timeout_at(deadline, &mut *task).await {
            Ok(Ok(())) => true,
            Ok(Err(error)) => {
                tracing::warn!("media pipeline task failed: {error}");
                false
            }
            Err(_) => {
                tracing::warn!(
                    "media pipeline exceeded its shared shutdown deadline; aborting and detaching async workers"
                );
                for child in &self.children {
                    child.abort();
                }
                task.abort();
                false
            }
        };
        self.task.take();
        clean
    }

    fn abort_children(&self) {
        for child in &self.children {
            child.abort();
        }
    }
}

impl Drop for OwnedPipeline {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(true);
        }
        self.abort_children();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

use async_trait::async_trait;
use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
};

use super::{Config, Output, PIPELINE_SHUTDOWN_TIMEOUT, Runtime, ServiceHandle, Snapshot};
use crate::{
    media::codec,
    peer_session,
    service_host::{HostedService, ShutdownContext},
};

const COMMAND_CAPACITY: usize = 32;
const EVENT_CAPACITY: usize = 32;

/// Owns the screen-share actor and every capture/codec pipeline below it.
pub(crate) struct ScreenShareService {
    shutdown_tx: watch::Sender<Option<tokio::time::Instant>>,
    task: Option<JoinHandle<Result<(), String>>>,
    handle: ServiceHandle,
}

impl ScreenShareService {
    pub(crate) fn start(
        initial_config: Config,
        sessions: peer_session::ServiceHandle,
        codecs: codec::ServiceHandle,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (config_tx, config_rx) = watch::channel(initial_config);
        let (snapshot_tx, snapshot_rx) = watch::channel(Snapshot::default());
        let (event_tx, _) = broadcast::channel(EVENT_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(None);
        let output = Output::new(snapshot_tx, event_tx.clone(), sessions.subscribe());
        let runtime =
            Runtime::new(command_rx, config_rx, sessions.clone(), codecs, output, shutdown_rx);
        let task = tokio::spawn(runtime.run());
        Self {
            shutdown_tx,
            task: Some(task),
            handle: ServiceHandle::new(command_tx, config_tx, snapshot_rx, event_tx),
        }
    }

    async fn shutdown_until(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), ShutdownError> {
        self.shutdown_tx.send_replace(Some(deadline));
        let Some(task) = self.task.as_mut() else {
            return Ok(());
        };
        let result = match tokio::time::timeout_at(deadline, &mut *task).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(error))) => Err(ShutdownError::UnexpectedExit(error)),
            Ok(Err(error)) => Err(ShutdownError::TaskFailed(error.to_string())),
            Err(_) => {
                // Tokio cancellation is cooperative. In particular, an in-flight
                // synchronous WGC setup call cannot observe the abort until it
                // returns. Drop the join handle at the advertised deadline; the
                // task then unwinds and delegates capture cleanup off-runtime.
                task.abort();
                Err(ShutdownError::DeadlineExceeded)
            }
        };
        self.task.take();
        result
    }

    fn deadline(context: ShutdownContext) -> tokio::time::Instant {
        context.bounded_deadline(PIPELINE_SHUTDOWN_TIMEOUT)
    }

    fn cancel_now(&mut self) {
        self.shutdown_tx.send_replace(Some(tokio::time::Instant::now()));
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[async_trait]
impl HostedService for ScreenShareService {
    const NAME: &'static str = "screen sharing";

    type ServiceHandle = ServiceHandle;
    type Error = ShutdownError;

    fn service_handle(&self) -> Self::ServiceHandle {
        self.handle.clone()
    }

    fn prepare_shutdown(&mut self, context: ShutdownContext) {
        self.shutdown_tx.send_replace(Some(Self::deadline(context)));
    }

    async fn shutdown(&mut self, context: ShutdownContext) -> Result<(), Self::Error> {
        self.shutdown_until(Self::deadline(context)).await
    }

    fn cancel(&mut self) {
        self.cancel_now();
    }
}

impl Drop for ScreenShareService {
    fn drop(&mut self) {
        self.cancel_now();
    }
}

impl std::fmt::Debug for ScreenShareService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ScreenShareService").finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ShutdownError {
    #[error("actor stopped unexpectedly: {0}")]
    UnexpectedExit(String),
    #[error("actor task failed: {0}")]
    TaskFailed(String),
    #[error("shutdown deadline exceeded")]
    DeadlineExceeded,
}

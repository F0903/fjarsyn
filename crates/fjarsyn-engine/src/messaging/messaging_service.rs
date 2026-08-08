use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::sync::{mpsc, watch};

use super::{Error, ServiceHandle, Store, actor::Actor, transport::SessionMessaging};
use crate::{
    peer_session,
    service_host::{HostedService, ShutdownContext},
};

#[derive(Debug, Clone)]
pub(crate) struct Limits {
    /// Maximum time an accepted command may wait in the actor queue before it
    /// is rejected without side effects.
    pub(crate) command_start_timeout: Duration,
    /// One aggregate deadline for draining mandatory events and joining the
    /// actor during explicit shutdown.
    pub(crate) shutdown_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            command_start_timeout: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(10),
        }
    }
}

pub(crate) struct Config {
    pub store: Arc<dyn Store>,
    pub sessions: peer_session::ServiceHandle,
    /// The single bounded, mandatory event stream owned by the peer-session
    /// service. Application composition creates this channel before either
    /// service starts and gives the sender to the peer-session service config.
    pub session_events: mpsc::Receiver<peer_session::Event>,
    pub limits: Limits,
}

pub(crate) struct MessagingService {
    handle: ServiceHandle,
    shutdown_tx: watch::Sender<bool>,
    shutdown_timeout: std::time::Duration,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl MessagingService {
    pub(crate) async fn start(config: Config) -> Result<Self, Error> {
        let session_transport: Arc<dyn SessionMessaging> = Arc::new(config.sessions);
        Self::start_with_transport_and_limits(
            config.store,
            session_transport,
            config.session_events,
            config.limits,
        )
        .await
    }

    #[cfg(test)]
    pub(super) async fn start_with_transport(
        store: Arc<dyn Store>,
        sessions: Arc<dyn SessionMessaging>,
        session_events: mpsc::Receiver<peer_session::Event>,
    ) -> Result<Self, Error> {
        Self::start_with_transport_and_limits(store, sessions, session_events, Limits::default())
            .await
    }

    pub(super) async fn start_with_transport_and_limits(
        store: Arc<dyn Store>,
        sessions: Arc<dyn SessionMessaging>,
        session_events: mpsc::Receiver<peer_session::Event>,
        limits: Limits,
    ) -> Result<Self, Error> {
        // A process can stop after SCTP accepted a frame but before the `Sent`
        // transition commits. Never retry it into a later session, and never
        // make the false claim that delivery definitely failed.
        store.mark_all_pending_unknown().await?;
        let initial_snapshot = Actor::load_snapshot(&store).await?;

        let (actor, channels) = Actor::new(store, sessions, session_events, initial_snapshot);
        let task = tokio::spawn(actor.run());

        Ok(Self {
            handle: ServiceHandle::new(
                channels.command_tx,
                channels.snapshot_rx,
                channels.event_tx,
                limits.command_start_timeout,
            ),
            shutdown_tx: channels.shutdown_tx,
            shutdown_timeout: limits.shutdown_timeout,
            task: Some(task),
        })
    }
}

#[async_trait]
impl HostedService for MessagingService {
    const NAME: &'static str = "messaging";

    type ServiceHandle = ServiceHandle;
    type Error = Error;

    fn service_handle(&self) -> Self::ServiceHandle {
        self.handle.clone()
    }

    async fn shutdown(&mut self, context: ShutdownContext) -> Result<(), Self::Error> {
        let _ = self.shutdown_tx.send(true);
        let Some(task) = self.task.as_mut() else { return Ok(()) };
        let deadline = context.bounded_deadline(self.shutdown_timeout);
        let result = match tokio::time::timeout_at(deadline, &mut *task).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(reason))) => Err(Error::TaskFailed(reason)),
            Ok(Err(error)) => Err(Error::TaskFailed(error.to_string())),
            Err(_) => {
                // Do not await after the absolute deadline. Dropping the
                // aborted handle below detaches cooperative actor cleanup.
                task.abort();
                Err(Error::ShutdownTimeout)
            }
        };
        self.task.take();
        result
    }

    fn cancel(&mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl fmt::Debug for MessagingService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessagingService").finish_non_exhaustive()
    }
}

impl Drop for MessagingService {
    fn drop(&mut self) {
        self.cancel();
    }
}

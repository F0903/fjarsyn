use std::{fmt, time::Duration};

use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant as TokioInstant,
};

use super::{
    config::Config, limits::negotiation_limits, orchestration::Runtime,
    service_handle::ServiceHandle,
};
use crate::{
    identity::{LocalPeerIdentity, PeerId, StoredIdentityKeypair},
    peer_session::{Error, negotiation},
    service_host::{HostedService, ShutdownContext},
};

pub struct PeerSessionService {
    local_peer_id: PeerId,
    identity: LocalPeerIdentity,
    signaling_port: u16,
    handle: ServiceHandle,
    listener: Option<negotiation::Listener>,
    task: Option<JoinHandle<()>>,
    shutdown_timeout: Duration,
    shutdown_tx: watch::Sender<Option<TokioInstant>>,
    shutdown_complete_rx: Option<oneshot::Receiver<Result<(), Error>>>,
}

impl fmt::Debug for PeerSessionService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PeerSessionService")
            .field("local_peer_id", &self.local_peer_id)
            .field("signaling_port", &self.signaling_port)
            .finish_non_exhaustive()
    }
}

impl PeerSessionService {
    pub async fn start(config: Config) -> Result<Self, Error> {
        let local_peer_id = match config.local_peer_id.clone() {
            Some(peer_id) => peer_id,
            None => PeerId::new(uuid::Uuid::new_v4().to_string())?,
        };
        let identity = match config.identity_keypair.as_ref() {
            Some(stored) => LocalPeerIdentity::from_stored(stored)
                .map_err(|error| Error::Protocol(error.to_string()))?,
            None => LocalPeerIdentity::generate(),
        };
        let negotiation_limits = negotiation_limits(&config.limits)?;
        let negotiation = negotiation::Service::new(
            local_peer_id.clone(),
            identity.clone(),
            config.trusted_peers.clone(),
            config.endpoints.clone(),
            negotiation_limits.clone(),
        );
        let (incoming_tx, incoming_rx) =
            mpsc::channel(config.limits.max_signaling_connections.max(1));
        let listener = negotiation::Listener::bind(
            config.signaling_port,
            local_peer_id.clone(),
            identity.clone(),
            config.trusted_peers.clone(),
            negotiation_limits.clone(),
            incoming_tx,
        )
        .await?;
        let signaling_port = listener.port();
        let listener_failure_rx = listener.failure_receiver();
        let (shutdown_tx, shutdown_rx) = watch::channel(None);
        let (shutdown_complete_tx, shutdown_complete_rx) = oneshot::channel();
        let shutdown_timeout = config.limits.shutdown_timeout;
        let (runtime, handle) = Runtime::new(
            local_peer_id.clone(),
            config,
            negotiation,
            incoming_rx,
            listener_failure_rx,
            shutdown_rx,
            shutdown_complete_tx,
        );
        let task = tokio::spawn(runtime.run());

        Ok(Self {
            local_peer_id,
            identity,
            signaling_port,
            handle,
            listener: Some(listener),
            task: Some(task),
            shutdown_timeout,
            shutdown_tx,
            shutdown_complete_rx: Some(shutdown_complete_rx),
        })
    }

    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    pub fn local_public_key(&self) -> String {
        self.identity.public_key_base64()
    }

    pub fn stored_identity_keypair(&self) -> StoredIdentityKeypair {
        self.identity.to_stored()
    }

    pub fn signaling_port(&self) -> u16 {
        self.signaling_port
    }

    async fn shutdown_runtime(&mut self, context: ShutdownContext) -> Result<(), Error> {
        let deadline = context.bounded_deadline(self.shutdown_timeout);
        self.shutdown_tx.send_replace(Some(deadline));
        let Self { listener, task, shutdown_complete_rx, .. } = self;
        let task = task.as_mut().ok_or(Error::ServiceStopped)?;
        let shutdown_complete = shutdown_complete_rx.as_mut().ok_or(Error::ServiceStopped)?;

        let graceful = {
            let listener_shutdown = async {
                if let Some(listener) = listener.as_mut() {
                    listener.shutdown_until(deadline).await
                } else {
                    Ok(())
                }
            };
            let runtime_shutdown = async {
                let runtime_result =
                    (&mut *shutdown_complete).await.map_err(|_| Error::ResponseDropped);
                let join_result = (&mut *task).await;
                if join_result.is_err() {
                    return Err(Error::ServiceStopped);
                }
                runtime_result?
            };
            tokio::time::timeout_at(deadline, async {
                let (listener_result, runtime_result) =
                    tokio::join!(listener_shutdown, runtime_shutdown);
                runtime_result.and(listener_result)
            })
            .await
        };

        match graceful {
            Ok(result) => {
                self.listener.take();
                self.task.take();
                self.shutdown_complete_rx.take();
                result
            }
            Err(_) => {
                // The absolute deadline has expired. Cancellation remains
                // synchronous; awaiting either task here would restart an
                // unbounded cleanup tail after the shared deadline fence.
                self.cancel_runtime();
                Err(Error::ShutdownTimeout)
            }
        }
    }

    /// Synchronously cancels the listener and runtime when awaited shutdown is
    /// no longer possible. Dropping the runtime future in turn aborts every
    /// registered session actor through its owned session entries.
    fn cancel_runtime(&mut self) {
        self.shutdown_tx.send_replace(Some(TokioInstant::now()));
        drop(self.listener.take());
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.shutdown_complete_rx.take();
    }
}

#[async_trait::async_trait]
impl HostedService for PeerSessionService {
    const NAME: &'static str = "peer sessions";

    type ServiceHandle = ServiceHandle;
    type Error = Error;

    fn service_handle(&self) -> Self::ServiceHandle {
        self.handle.clone()
    }

    async fn shutdown(&mut self, context: ShutdownContext) -> Result<(), Self::Error> {
        self.shutdown_runtime(context).await
    }

    fn cancel(&mut self) {
        self.cancel_runtime();
    }
}

impl Drop for PeerSessionService {
    fn drop(&mut self) {
        self.cancel_runtime();
    }
}

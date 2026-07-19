use std::{
    collections::{HashMap, HashSet},
    fmt,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant as TokioInstant,
};

use super::{
    EncodedVideoSink, MessageId, PeerId, PeerSessionError, PeerSessionEvent, PeerSessionPhase,
    PeerSessionServiceSnapshot, RemoteVideoSource, SessionCloseReason, SessionId, ShareId,
    actor::{
        self, RestartAttachment, SessionActorConfig, SessionActorHandle, SessionCommand,
        SessionRole, SessionTerminal, SessionUpdate,
    },
    negotiation::{
        IncomingNegotiation, NegotiationConnection, NegotiationIntent, NegotiationLimits,
        NegotiationListener, NegotiationService,
    },
    rtc::RtcConfig,
};
use crate::identity::{LocalPeerIdentity, StoredIdentityKeypair, TrustedPeerIdentity};

#[async_trait]
pub trait TrustedPeerResolver: Send + Sync {
    async fn trusted_peer(
        &self,
        peer_id: &PeerId,
    ) -> Result<Option<TrustedPeerIdentity>, PeerSessionError>;
}

#[async_trait]
pub trait PeerEndpointResolver: Send + Sync {
    /// Returns one immutable, ordered snapshot of the peer's current endpoint
    /// hints. These addresses are unauthenticated discovery data; successful
    /// signaling authentication, never endpoint selection, establishes peer
    /// identity.
    async fn endpoint_hints_for(
        &self,
        peer_id: &PeerId,
    ) -> Result<Arc<[SocketAddr]>, PeerSessionError>;
}

#[derive(Debug, Clone)]
pub struct PeerSessionLimits {
    pub max_sessions: usize,
    pub service_command_capacity: usize,
    pub session_command_capacity: usize,
    pub session_update_capacity: usize,
    pub event_capacity: usize,
    pub video_input_capacity: usize,
    pub remote_video_capacity: usize,
    pub max_message_bytes: usize,
    pub max_data_message_bytes: usize,
    pub max_signaling_frame_bytes: usize,
    pub signaling_queue_capacity: usize,
    pub signaling_replay_capacity: usize,
    pub max_signaling_connections: usize,
    pub max_signaling_connections_per_ip: usize,
    pub signaling_auth_global_burst: usize,
    pub signaling_auth_global_refill_interval: Duration,
    pub signaling_auth_per_ip_burst: usize,
    pub signaling_auth_per_ip_refill_interval: Duration,
    pub max_signaling_auth_tracked_ips: usize,
    pub max_endpoint_attempts: usize,
    pub endpoint_attempt_timeout: Duration,
    pub signaling_handshake_timeout: Duration,
    pub signaling_idle_timeout: Duration,
    pub signaling_max_message_age: Duration,
    pub signaling_max_clock_skew: Duration,
    pub max_ice_candidates_per_generation: usize,
    pub request_timeout: Duration,
    pub negotiation_timeout: Duration,
    pub shutdown_timeout: Duration,
    pub event_delivery_timeout: Duration,
    pub pre_ready_data_capacity: usize,
    pub service_operation_timeout: Duration,
    pub disconnected_grace: Duration,
    pub ice_restart_timeout: Duration,
    pub rtc_operation_timeout: Duration,
    pub max_remote_timestamp_age: Duration,
    pub max_remote_clock_skew: Duration,
}

impl Default for PeerSessionLimits {
    fn default() -> Self {
        Self {
            max_sessions: 8,
            service_command_capacity: 64,
            session_command_capacity: 64,
            session_update_capacity: 256,
            event_capacity: 256,
            video_input_capacity: 3,
            // Keep the initial SPS/PPS/IDR window available while the
            // authenticated ShareStarted projection reaches native media.
            remote_video_capacity: 64,
            max_message_bytes: 12 * 1024,
            max_data_message_bytes: 16 * 1024,
            max_signaling_frame_bytes: 256 * 1024,
            signaling_queue_capacity: 64,
            signaling_replay_capacity: 4096,
            max_signaling_connections: 32,
            max_signaling_connections_per_ip: 4,
            signaling_auth_global_burst: 64,
            signaling_auth_global_refill_interval: Duration::from_millis(100),
            signaling_auth_per_ip_burst: 8,
            signaling_auth_per_ip_refill_interval: Duration::from_millis(500),
            max_signaling_auth_tracked_ips: 4096,
            max_endpoint_attempts: 6,
            endpoint_attempt_timeout: Duration::from_secs(2),
            signaling_handshake_timeout: Duration::from_secs(10),
            signaling_idle_timeout: Duration::from_secs(60),
            signaling_max_message_age: Duration::from_secs(5 * 60),
            signaling_max_clock_skew: Duration::from_secs(30),
            max_ice_candidates_per_generation: 256,
            request_timeout: Duration::from_secs(30),
            negotiation_timeout: Duration::from_secs(45),
            shutdown_timeout: Duration::from_secs(5),
            event_delivery_timeout: Duration::from_secs(2),
            pre_ready_data_capacity: 32,
            service_operation_timeout: Duration::from_secs(15),
            disconnected_grace: Duration::from_secs(5),
            ice_restart_timeout: Duration::from_secs(20),
            rtc_operation_timeout: Duration::from_secs(2),
            max_remote_timestamp_age: Duration::from_secs(5 * 60),
            max_remote_clock_skew: Duration::from_secs(30),
        }
    }
}

#[derive(Clone)]
pub struct PeerSessionServiceConfig {
    pub local_peer_id: Option<PeerId>,
    pub identity_keypair: Option<StoredIdentityKeypair>,
    pub trusted_peers: Arc<dyn TrustedPeerResolver>,
    pub endpoints: Arc<dyn PeerEndpointResolver>,
    pub signaling_port: u16,
    pub ice_servers: Vec<String>,
    pub max_depacket_latency: Duration,
    pub limits: PeerSessionLimits,
    /// Mandatory, ordered persistence/event consumer. If this bounded queue
    /// closes or fills, all sessions are failed rather than dropping events.
    pub mandatory_event_sink: Option<mpsc::Sender<PeerSessionEvent>>,
}

impl PeerSessionServiceConfig {
    pub fn new(
        trusted_peers: Arc<dyn TrustedPeerResolver>,
        endpoints: Arc<dyn PeerEndpointResolver>,
    ) -> Self {
        Self {
            local_peer_id: None,
            identity_keypair: None,
            trusted_peers,
            endpoints,
            signaling_port: 0,
            ice_servers: Vec::new(),
            max_depacket_latency: Duration::from_millis(100),
            limits: PeerSessionLimits::default(),
            mandatory_event_sink: None,
        }
    }
}

impl fmt::Debug for PeerSessionServiceConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PeerSessionServiceConfig")
            .field("local_peer_id", &self.local_peer_id)
            .field("has_identity_keypair", &self.identity_keypair.is_some())
            .field("signaling_port", &self.signaling_port)
            .field("ice_servers", &self.ice_servers)
            .field("max_depacket_latency", &self.max_depacket_latency)
            .field("limits", &self.limits)
            .field("has_mandatory_event_sink", &self.mandatory_event_sink.is_some())
            .finish_non_exhaustive()
    }
}

pub struct PeerSessionService {
    local_peer_id: PeerId,
    identity: LocalPeerIdentity,
    signaling_port: u16,
    handle: PeerSessionServiceHandle,
    listener: Option<NegotiationListener>,
    task: Option<JoinHandle<()>>,
    shutdown_timeout: Duration,
    shutdown_tx: watch::Sender<Option<TokioInstant>>,
    shutdown_complete_rx: Option<oneshot::Receiver<Result<(), PeerSessionError>>>,
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
    pub async fn start(config: PeerSessionServiceConfig) -> Result<Self, PeerSessionError> {
        let local_peer_id = match config.local_peer_id.clone() {
            Some(peer_id) => peer_id,
            None => PeerId::new(uuid::Uuid::new_v4().to_string())?,
        };
        let identity = match config.identity_keypair.as_ref() {
            Some(stored) => LocalPeerIdentity::from_stored(stored)
                .map_err(|error| PeerSessionError::Protocol(error.to_string()))?,
            None => LocalPeerIdentity::generate(),
        };
        let negotiation_limits = negotiation_limits(&config.limits)?;
        let negotiation = NegotiationService::new(
            local_peer_id.clone(),
            identity.clone(),
            config.trusted_peers.clone(),
            config.endpoints.clone(),
            config.limits.max_endpoint_attempts,
            config.limits.endpoint_attempt_timeout,
            negotiation_limits.clone(),
        );
        let (incoming_tx, incoming_rx) =
            mpsc::channel(config.limits.max_signaling_connections.max(1));
        let listener = NegotiationListener::bind(
            config.signaling_port,
            local_peer_id.clone(),
            identity.clone(),
            config.trusted_peers.clone(),
            negotiation_limits.clone(),
            incoming_tx,
        )
        .await?;
        let signaling_port = listener.port();
        let (command_tx, command_rx) = mpsc::channel(config.limits.service_command_capacity.max(1));
        let (update_tx, update_rx) = mpsc::channel(config.limits.session_update_capacity.max(1));
        let (terminal_tx, terminal_rx) = mpsc::unbounded_channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(PeerSessionServiceSnapshot::default());
        let (event_tx, _) = broadcast::channel(config.limits.event_capacity.max(1));
        let (shutdown_tx, shutdown_rx) = watch::channel(None);
        let (shutdown_complete_tx, shutdown_complete_rx) = oneshot::channel();
        let operation_timeout =
            config.limits.service_operation_timeout.saturating_add(Duration::from_secs(1));
        let shutdown_timeout = config.limits.shutdown_timeout;
        let handle = PeerSessionServiceHandle {
            command_tx,
            snapshot_rx,
            event_tx: event_tx.clone(),
            operation_timeout,
        };
        let session_id_retention = config.limits.signaling_max_message_age;
        let recent_session_capacity = config.limits.signaling_replay_capacity;
        let runtime = ServiceRuntime {
            local_peer_id: local_peer_id.clone(),
            trusted_peers: config.trusted_peers,
            negotiation,
            ice_servers: config.ice_servers,
            max_depacket_latency: config.max_depacket_latency,
            limits: config.limits,
            negotiation_limits,
            sessions: HashMap::new(),
            peers: HashMap::new(),
            suspended_peers: HashMap::new(),
            recent_session_ids: RecentSessionIds::new(
                session_id_retention,
                recent_session_capacity,
            ),
            command_rx,
            incoming_rx,
            update_tx,
            update_rx,
            terminal_tx,
            terminal_rx,
            snapshot_tx,
            event_tx,
            mandatory_event_sink: config.mandatory_event_sink,
            mandatory_event_sink_failed: false,
            shutdown_rx,
            shutdown_complete_tx: Some(shutdown_complete_tx),
        };
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

    pub fn handle(&self) -> PeerSessionServiceHandle {
        self.handle.clone()
    }

    pub async fn shutdown(mut self) -> Result<(), PeerSessionError> {
        let deadline = TokioInstant::now() + self.shutdown_timeout;
        self.shutdown_tx.send_replace(Some(deadline));
        let mut listener = self.listener.take();
        let mut task = self.task.take().ok_or(PeerSessionError::ServiceStopped)?;
        let mut shutdown_complete =
            self.shutdown_complete_rx.take().ok_or(PeerSessionError::ServiceStopped)?;

        let graceful = {
            let listener_shutdown = async {
                if let Some(listener) = listener.as_mut() {
                    listener.shutdown().await;
                }
            };
            let runtime_shutdown = async {
                let runtime_result =
                    (&mut shutdown_complete).await.map_err(|_| PeerSessionError::ResponseDropped);
                let join_result = (&mut task).await;
                if join_result.is_err() {
                    return Err(PeerSessionError::ServiceStopped);
                }
                runtime_result?
            };
            tokio::time::timeout_at(deadline, async {
                let (_, runtime_result) = tokio::join!(listener_shutdown, runtime_shutdown);
                runtime_result
            })
            .await
        };

        match graceful {
            Ok(result) => result,
            Err(_) => {
                if let Some(listener) = listener.as_mut() {
                    listener.abort_and_join().await;
                }
                task.abort();
                let _ = task.await;
                Err(PeerSessionError::ShutdownTimeout)
            }
        }
    }
}

impl Drop for PeerSessionService {
    fn drop(&mut self) {
        self.shutdown_tx.send_replace(Some(TokioInstant::now() + self.shutdown_timeout));
    }
}

static NEXT_TRUST_BARRIER_OWNER_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TrustBarrierOwnerId(u64);

impl TrustBarrierOwnerId {
    pub(crate) fn allocate() -> Self {
        Self(
            NEXT_TRUST_BARRIER_OWNER_ID
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| next.checked_add(1))
                .expect("trust barrier owner ID space exhausted"),
        )
    }
}

#[derive(Debug, Clone)]
pub struct PeerSessionServiceHandle {
    command_tx: mpsc::Sender<ServiceCommand>,
    snapshot_rx: watch::Receiver<PeerSessionServiceSnapshot>,
    event_tx: broadcast::Sender<PeerSessionEvent>,
    operation_timeout: Duration,
}

impl PeerSessionServiceHandle {
    pub fn snapshot(&self) -> PeerSessionServiceSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<PeerSessionServiceSnapshot> {
        self.snapshot_rx.clone()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<PeerSessionEvent> {
        self.event_tx.subscribe()
    }

    pub fn events(&self) -> broadcast::Receiver<PeerSessionEvent> {
        self.subscribe_events()
    }

    pub async fn connect(&self, peer_id: PeerId) -> Result<SessionId, PeerSessionError> {
        self.send_command(|reply| ServiceCommand::Connect { peer_id, reply }).await
    }

    /// Idempotently installs the application-owned trust-mutation barrier.
    /// The first installation prevents admission and closes an authenticated
    /// session before returning; retries reassert the same barrier.
    pub(crate) async fn ensure_trust_suspended(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), PeerSessionError> {
        self.send_command(|reply| ServiceCommand::EnsureTrustSuspended { peer_id, owner_id, reply })
            .await
    }

    /// Idempotently releases the application-owned trust-mutation barrier.
    pub(crate) async fn release_trust_suspension(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), PeerSessionError> {
        self.send_command(|reply| ServiceCommand::ReleaseTrustSuspension {
            peer_id,
            owner_id,
            reply,
        })
        .await
    }

    pub async fn accept(&self, session_id: SessionId) -> Result<(), PeerSessionError> {
        self.session_command(session_id, SessionCommand::Accept).await
    }

    pub async fn reject(
        &self,
        session_id: SessionId,
        reason: impl Into<String>,
    ) -> Result<(), PeerSessionError> {
        let reason = reason.into();
        self.session_command(session_id, |reply| SessionCommand::Reject { reason, reply }).await
    }

    pub async fn disconnect(&self, session_id: SessionId) -> Result<(), PeerSessionError> {
        self.session_command(session_id, SessionCommand::Disconnect).await
    }

    pub async fn send_message(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        body: impl Into<String>,
        sent_at: DateTime<Utc>,
    ) -> Result<(), PeerSessionError> {
        let body = body.into();
        self.session_command(session_id, |reply| SessionCommand::SendMessage {
            message_id,
            body,
            sent_at,
            reply,
        })
        .await
    }

    pub async fn send_receipt(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), PeerSessionError> {
        self.session_command(session_id, |reply| SessionCommand::SendReceipt {
            message_id,
            received_at,
            reply,
        })
        .await
    }

    pub async fn start_screen_share(
        &self,
        session_id: SessionId,
    ) -> Result<ShareId, PeerSessionError> {
        self.session_command(session_id, SessionCommand::StartShare).await
    }

    pub async fn stop_screen_share(
        &self,
        session_id: SessionId,
        share_id: ShareId,
    ) -> Result<(), PeerSessionError> {
        self.session_command(session_id, |reply| SessionCommand::StopShare { share_id, reply })
            .await
    }

    #[cfg(test)]
    async fn force_ice_restart(&self, session_id: SessionId) -> Result<(), PeerSessionError> {
        self.session_command(session_id, SessionCommand::ForceIceRestart).await
    }

    #[cfg(test)]
    async fn committed_transport_generation(
        &self,
        session_id: SessionId,
    ) -> Result<u64, PeerSessionError> {
        self.session_command(session_id, SessionCommand::CommittedTransportGeneration)
            .await
            .map(super::restart::TransportGeneration::value)
    }

    pub async fn encoded_video_sink(
        &self,
        session_id: SessionId,
        share_id: ShareId,
    ) -> Result<EncodedVideoSink, PeerSessionError> {
        self.send_command(|reply| ServiceCommand::EncodedVideoSink { session_id, share_id, reply })
            .await
    }

    pub async fn subscribe_remote_video(
        &self,
        session_id: SessionId,
    ) -> Result<RemoteVideoSource, PeerSessionError> {
        self.send_command(|reply| ServiceCommand::RemoteVideoSource { session_id, reply }).await
    }

    async fn session_command<T>(
        &self,
        session_id: SessionId,
        build: impl FnOnce(oneshot::Sender<Result<T, PeerSessionError>>) -> SessionCommand,
    ) -> Result<T, PeerSessionError> {
        self.send_command(|reply| ServiceCommand::Session { session_id, command: build(reply) })
            .await
    }

    async fn send_command<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, PeerSessionError>>) -> ServiceCommand,
    ) -> Result<T, PeerSessionError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        tokio::time::timeout(self.operation_timeout, self.command_tx.send(build(reply_tx)))
            .await
            .map_err(|_| PeerSessionError::OperationTimeout)?
            .map_err(|_| PeerSessionError::ServiceStopped)?;
        // Once accepted by the service queue, mutations have definitive completion
        // semantics. Every underlying network/RTC operation is independently bounded,
        // so returning a caller timeout here would allow a queued mutation to run later.
        reply_rx.await.map_err(|_| PeerSessionError::ResponseDropped)?
    }
}

#[derive(Debug)]
enum ServiceCommand {
    Connect {
        peer_id: PeerId,
        reply: oneshot::Sender<Result<SessionId, PeerSessionError>>,
    },
    EnsureTrustSuspended {
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
        reply: oneshot::Sender<Result<(), PeerSessionError>>,
    },
    ReleaseTrustSuspension {
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
        reply: oneshot::Sender<Result<(), PeerSessionError>>,
    },
    Session {
        session_id: SessionId,
        command: SessionCommand,
    },
    EncodedVideoSink {
        session_id: SessionId,
        share_id: ShareId,
        reply: oneshot::Sender<Result<EncodedVideoSink, PeerSessionError>>,
    },
    RemoteVideoSource {
        session_id: SessionId,
        reply: oneshot::Sender<Result<RemoteVideoSource, PeerSessionError>>,
    },
}

struct SessionEntry {
    handle: SessionActorHandle,
    task: JoinHandle<()>,
}

impl Drop for SessionEntry {
    fn drop(&mut self) {
        // JoinHandle normally detaches on drop. Aborting here makes unexpected
        // ServiceRuntime cancellation fail closed instead of orphaning an actor.
        self.task.abort();
    }
}

struct ServiceRuntime {
    local_peer_id: PeerId,
    trusted_peers: Arc<dyn TrustedPeerResolver>,
    negotiation: NegotiationService,
    ice_servers: Vec<String>,
    max_depacket_latency: Duration,
    limits: PeerSessionLimits,
    negotiation_limits: NegotiationLimits,
    sessions: HashMap<SessionId, SessionEntry>,
    peers: HashMap<PeerId, SessionId>,
    suspended_peers: HashMap<PeerId, HashSet<TrustBarrierOwnerId>>,
    recent_session_ids: RecentSessionIds,
    command_rx: mpsc::Receiver<ServiceCommand>,
    incoming_rx: mpsc::Receiver<IncomingNegotiation>,
    update_tx: mpsc::Sender<SessionUpdate>,
    update_rx: mpsc::Receiver<SessionUpdate>,
    terminal_tx: mpsc::UnboundedSender<SessionTerminal>,
    terminal_rx: mpsc::UnboundedReceiver<SessionTerminal>,
    snapshot_tx: watch::Sender<PeerSessionServiceSnapshot>,
    event_tx: broadcast::Sender<PeerSessionEvent>,
    mandatory_event_sink: Option<mpsc::Sender<PeerSessionEvent>>,
    mandatory_event_sink_failed: bool,
    shutdown_rx: watch::Receiver<Option<TokioInstant>>,
    shutdown_complete_tx: Option<oneshot::Sender<Result<(), PeerSessionError>>>,
}

impl ServiceRuntime {
    async fn run(mut self) {
        let mut snapshot_tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            let mandatory_sink = self.mandatory_event_sink.clone();
            tokio::select! {
                biased;
                changed = self.shutdown_rx.changed() => {
                    let deadline = if changed.is_ok() {
                        *self.shutdown_rx.borrow_and_update()
                    } else {
                        *self.shutdown_rx.borrow()
                    }
                    .unwrap_or_else(|| TokioInstant::now() + self.limits.shutdown_timeout);
                    self.complete_shutdown(deadline).await;
                    break;
                }
                _ = wait_for_mandatory_sink_closed(mandatory_sink) => {
                    self.fail_mandatory_event_sink();
                }
                command = self.command_rx.recv() => {
                    match command {
                        Some(command) => self.handle_command(command).await,
                        None => {
                            let deadline = TokioInstant::now() + self.limits.shutdown_timeout;
                            self.complete_shutdown(deadline).await;
                            break;
                        }
                    }
                }
                incoming = self.incoming_rx.recv() => {
                    if let Some(incoming) = incoming {
                        let mut shutdown_rx = self.shutdown_rx.clone();
                        tokio::select! {
                            biased;
                            _ = receive_shutdown_deadline(&mut shutdown_rx) => {}
                            _ = self.handle_incoming(incoming) => {}
                        }
                    }
                }
                update = self.update_rx.recv() => {
                    if let Some(update) = update {
                        self.handle_update(update).await;
                    }
                }
                terminal = self.terminal_rx.recv() => {
                    if let Some(terminal) = terminal {
                        self.handle_terminal(terminal).await;
                    }
                }
                _ = snapshot_tick.tick() => self.publish_snapshot(),
            }
        }
    }

    async fn complete_shutdown(&mut self, deadline: TokioInstant) {
        self.command_rx.close();
        while let Ok(command) = self.command_rx.try_recv() {
            reply_service_error(command, PeerSessionError::ServiceStopped);
        }
        let result = self.shutdown_sessions(deadline).await;
        self.mandatory_event_sink.take();
        if let Some(reply) = self.shutdown_complete_tx.take() {
            let _ = reply.send(result);
        }
    }

    async fn handle_command(&mut self, command: ServiceCommand) {
        match command {
            ServiceCommand::Connect { peer_id, reply } => {
                let mut shutdown_rx = self.shutdown_rx.clone();
                let result = tokio::select! {
                    biased;
                    _ = receive_shutdown_deadline(&mut shutdown_rx) => {
                        Err(PeerSessionError::ServiceStopped)
                    }
                    result = self.connect(peer_id) => result,
                };
                let _ = reply.send(result);
            }
            ServiceCommand::EnsureTrustSuspended { peer_id, owner_id, reply } => {
                let first_owner = {
                    let owners = self.suspended_peers.entry(peer_id.clone()).or_default();
                    let first_owner = owners.is_empty();
                    owners.insert(owner_id);
                    first_owner
                };
                if first_owner {
                    self.terminate_suspended_peer(&peer_id).await;
                }
                let _ = reply.send(Ok(()));
            }
            ServiceCommand::ReleaseTrustSuspension { peer_id, owner_id, reply } => {
                let remove_peer = self.suspended_peers.get_mut(&peer_id).is_some_and(|owners| {
                    owners.remove(&owner_id);
                    owners.is_empty()
                });
                if remove_peer {
                    self.suspended_peers.remove(&peer_id);
                }
                let _ = reply.send(Ok(()));
            }
            ServiceCommand::Session { session_id, command } => {
                let Some(entry) = self.sessions.get(&session_id) else {
                    command.reply_error(PeerSessionError::SessionNotFound(session_id));
                    return;
                };
                if let Err(error) = entry.handle.command_tx().try_send(command) {
                    match error {
                        mpsc::error::TrySendError::Full(command) => {
                            command.reply_error(PeerSessionError::SessionBusy(session_id));
                        }
                        mpsc::error::TrySendError::Closed(command) => {
                            command.reply_error(PeerSessionError::ServiceStopped);
                        }
                    }
                }
            }
            ServiceCommand::EncodedVideoSink { session_id, share_id, reply } => {
                let result = self
                    .connected_entry(session_id)
                    .and_then(|entry| entry.handle.encoded_video_sink(share_id));
                let _ = reply.send(result);
            }
            ServiceCommand::RemoteVideoSource { session_id, reply } => {
                let result = self
                    .connected_entry(session_id)
                    .map(|entry| entry.handle.remote_video_source());
                let _ = reply.send(result);
            }
        }
    }

    async fn connect(&mut self, peer_id: PeerId) -> Result<SessionId, PeerSessionError> {
        tokio::time::timeout(self.limits.service_operation_timeout, self.connect_inner(peer_id))
            .await
            .map_err(|_| PeerSessionError::OperationTimeout)?
    }

    async fn connect_inner(&mut self, peer_id: PeerId) -> Result<SessionId, PeerSessionError> {
        if self.mandatory_event_sink_failed {
            return Err(PeerSessionError::Protocol(
                "reliable peer-session event delivery is unavailable".into(),
            ));
        }
        if peer_id == self.local_peer_id {
            return Err(PeerSessionError::Protocol("cannot connect to the local peer".into()));
        }
        if self.suspended_peers.contains_key(&peer_id) {
            return Err(PeerSessionError::PeerSuspended(peer_id));
        }
        if self.peers.contains_key(&peer_id) {
            return Err(PeerSessionError::SessionAlreadyExists(peer_id));
        }
        if self.sessions.len() >= self.limits.max_sessions {
            return Err(PeerSessionError::Protocol("session capacity reached".into()));
        }
        let session_id = loop {
            let candidate = SessionId::new();
            if !self.recent_session_ids.seen_or_remember(candidate, Instant::now()) {
                break candidate;
            }
        };
        let connection = self.negotiation.connect(session_id, peer_id.clone()).await?;
        self.insert_session(session_id, peer_id, SessionRole::Outgoing, connection).await?;
        Ok(session_id)
    }

    async fn handle_incoming(&mut self, incoming: IncomingNegotiation) {
        let reject_timeout =
            self.negotiation_limits.handshake_timeout.min(self.limits.service_operation_timeout);
        if self.mandatory_event_sink_failed {
            reject_connection(
                incoming.connection,
                "reliable peer-session event delivery is unavailable",
                reject_timeout,
            )
            .await;
            return;
        }
        if incoming.peer_id == self.local_peer_id {
            reject_connection(
                incoming.connection,
                "cannot connect to the local peer",
                reject_timeout,
            )
            .await;
            return;
        }
        if self.suspended_peers.contains_key(&incoming.peer_id) {
            reject_connection(incoming.connection, "peer identity is suspended", reject_timeout)
                .await;
            return;
        }
        let current_trusted_peer = tokio::time::timeout(
            self.limits.service_operation_timeout,
            self.trusted_peers.trusted_peer(&incoming.peer_id),
        )
        .await;
        let current_trusted_peer = match current_trusted_peer {
            Ok(Ok(Some(trusted_peer))) => trusted_peer,
            _ => {
                reject_connection(
                    incoming.connection,
                    "peer identity is no longer trusted",
                    reject_timeout,
                )
                .await;
                return;
            }
        };
        if current_trusted_peer.public_key != incoming.authenticated_public_key {
            reject_connection(
                incoming.connection,
                "peer identity changed during authentication",
                reject_timeout,
            )
            .await;
            return;
        }
        if let NegotiationIntent::Restart { generation } = incoming.intent {
            self.handle_incoming_restart(incoming, generation);
            return;
        }
        if self.recent_session_ids.seen_or_remember(incoming.session_id, Instant::now()) {
            reject_connection(
                incoming.connection,
                "session identifier was already used",
                reject_timeout,
            )
            .await;
            return;
        }

        let mut auto_accept = false;
        if let Some(existing_id) = self.peers.get(&incoming.peer_id).copied() {
            let existing_phase =
                self.sessions.get(&existing_id).map(|entry| entry.handle.snapshot().phase);
            match resolve_incoming_request(&self.local_peer_id, &incoming.peer_id, existing_phase) {
                IncomingRequestResolution::RejectExistingSession => {
                    reject_connection(
                        incoming.connection,
                        "a session with this peer already exists",
                        reject_timeout,
                    )
                    .await;
                    return;
                }
                IncomingRequestResolution::KeepOutgoing => {
                    reject_connection(
                        incoming.connection,
                        "simultaneous request superseded",
                        reject_timeout,
                    )
                    .await;
                    return;
                }
                IncomingRequestResolution::ReplaceAndAccept => {}
                IncomingRequestResolution::Prompt => unreachable!("existing session was present"),
            }

            if let Some(mut existing) = self.sessions.remove(&existing_id) {
                self.peers.remove(&incoming.peer_id);
                existing.handle.fail("simultaneous outgoing session was superseded");
                if tokio::time::timeout(self.limits.shutdown_timeout, &mut existing.task)
                    .await
                    .is_err()
                {
                    existing.task.abort();
                    let _ = (&mut existing.task).await;
                }
            } else {
                self.peers.remove(&incoming.peer_id);
            }
            auto_accept = true;
        } else if self.sessions.len() >= self.limits.max_sessions {
            reject_connection(incoming.connection, "session capacity reached", reject_timeout)
                .await;
            return;
        }

        let session_id = incoming.session_id;
        let peer_id = incoming.peer_id;
        if self.sessions.contains_key(&session_id) {
            reject_connection(incoming.connection, "session identifier collision", reject_timeout)
                .await;
            return;
        }
        if self
            .insert_session(session_id, peer_id.clone(), SessionRole::Incoming, incoming.connection)
            .await
            .is_ok()
        {
            if auto_accept {
                if let Some(entry) = self.sessions.get(&session_id) {
                    let (reply, _ignored) = oneshot::channel();
                    if entry.handle.command_tx().try_send(SessionCommand::Accept(reply)).is_err() {
                        entry.handle.fail("automatic simultaneous-connect acceptance failed");
                    }
                }
            } else {
                self.emit(PeerSessionEvent::IncomingRequest { session_id, peer_id }).await;
            }
        }
    }

    fn handle_incoming_restart(
        &mut self,
        incoming: IncomingNegotiation,
        generation: super::restart::TransportGeneration,
    ) {
        let Some(entry) = self.sessions.get(&incoming.session_id) else {
            discard_restart_connection(
                incoming.connection,
                "restart does not identify an active session",
            );
            return;
        };
        if entry.handle.snapshot().peer_id != incoming.peer_id
            || self.peers.get(&incoming.peer_id) != Some(&incoming.session_id)
        {
            discard_restart_connection(
                incoming.connection,
                "restart peer and session do not match",
            );
            return;
        }
        if !matches!(
            entry.handle.snapshot().phase,
            PeerSessionPhase::Connected | PeerSessionPhase::Reconnecting
        ) {
            discard_restart_connection(
                incoming.connection,
                "session is not eligible for ICE restart",
            );
            return;
        }

        let attachment = RestartAttachment { generation, connection: incoming.connection };
        if let Err(error) = entry.handle.try_attach_restart(attachment) {
            let attachment = *error;
            discard_restart_connection(
                attachment.connection,
                "session cannot accept restart signaling",
            );
        }
    }

    async fn insert_session(
        &mut self,
        session_id: SessionId,
        peer_id: PeerId,
        role: SessionRole,
        connection: NegotiationConnection,
    ) -> Result<(), PeerSessionError> {
        let remote_public_key = connection.authenticated_remote_public_key().to_owned();
        let rtc = RtcConfig {
            ice_servers: self.ice_servers.clone(),
            max_depacket_latency: self.max_depacket_latency,
            max_candidates_per_generation: self.limits.max_ice_candidates_per_generation,
            max_data_message_bytes: self.limits.max_data_message_bytes,
            operation_timeout: self.limits.rtc_operation_timeout,
        };
        let config = SessionActorConfig {
            session_id,
            remote_peer_id: peer_id.clone(),
            remote_public_key,
            role,
            connection: Some(connection),
            negotiation: self.negotiation.clone(),
            rtc,
            command_capacity: self.limits.session_command_capacity,
            media_capacity: self.limits.video_input_capacity,
            remote_video_capacity: self.limits.remote_video_capacity,
            max_message_bytes: self.limits.max_message_bytes,
            max_data_message_bytes: self.limits.max_data_message_bytes,
            request_timeout: self.limits.request_timeout,
            negotiation_timeout: self.limits.negotiation_timeout,
            event_delivery_timeout: self.limits.event_delivery_timeout,
            cleanup_timeout: self.limits.shutdown_timeout,
            pre_ready_data_capacity: self.limits.pre_ready_data_capacity.max(1),
            disconnected_grace: self.limits.disconnected_grace,
            ice_restart_timeout: self.limits.ice_restart_timeout,
            max_remote_timestamp_age: self.limits.max_remote_timestamp_age,
            max_remote_clock_skew: self.limits.max_remote_clock_skew,
        };
        let (handle, task) = actor::spawn(config, self.update_tx.clone(), self.terminal_tx.clone());
        self.peers.insert(peer_id, session_id);
        self.sessions.insert(session_id, SessionEntry { handle, task });
        self.publish_snapshot();
        Ok(())
    }

    async fn handle_update(&mut self, update: SessionUpdate) {
        match update {
            SessionUpdate::Event { generation, event } => {
                if self
                    .sessions
                    .get(&event.session_id())
                    .is_some_and(|entry| entry.handle.generation == generation)
                {
                    self.emit(event).await;
                }
            }
        }
    }

    async fn handle_terminal(&mut self, terminal: SessionTerminal) {
        // The actor sends its terminal marker only after every semantic update
        // send has completed. The separate terminal channel can nevertheless win
        // select first, so drain the ordered update queue before removing the actor.
        for update in drain_pending_session_updates(&mut self.update_rx) {
            self.handle_update(update).await;
        }
        let SessionTerminal { generation, session_id, peer_id, reason } = terminal;
        let current_generation =
            self.sessions.get(&session_id).map(|entry| entry.handle.generation);
        if current_generation != Some(generation) {
            return;
        }
        if let Some(mut entry) = self.sessions.remove(&session_id) {
            self.peers.remove(&peer_id);
            let _ = (&mut entry.task).await;
            self.publish_snapshot();
            self.emit(PeerSessionEvent::Closed {
                session_id,
                peer_id: peer_id.clone(),
                reason: reason.clone(),
            })
            .await;
        }
        tracing::debug!(%session_id, %peer_id, ?reason, "peer session removed");
    }

    async fn emit(&mut self, event: PeerSessionEvent) {
        let _ = self.event_tx.send(event.clone());
        let delivery_failed = self
            .mandatory_event_sink
            .as_ref()
            .is_some_and(|sink| sink.try_send(event.clone()).is_err());
        if delivery_failed {
            self.fail_mandatory_event_sink();
        }
    }

    fn fail_mandatory_event_sink(&mut self) {
        if self.mandatory_event_sink_failed {
            return;
        }
        self.mandatory_event_sink.take();
        self.mandatory_event_sink_failed = true;
        tracing::error!(
            "reliable peer-session event sink overflowed or closed; terminating sessions"
        );
        for entry in self.sessions.values() {
            entry.handle.fail("mandatory peer-session event delivery failed");
        }
    }

    async fn terminate_suspended_peer(&mut self, peer_id: &PeerId) {
        let Some(session_id) = self.peers.remove(peer_id) else {
            return;
        };
        let Some(mut entry) = self.sessions.remove(&session_id) else {
            return;
        };
        let generation = entry.handle.generation;
        let deadline = TokioInstant::now() + self.limits.shutdown_timeout;
        entry.handle.revoke_trust(deadline);
        if tokio::time::timeout_at(deadline, &mut entry.task).await.is_err() {
            tracing::warn!(%session_id, %peer_id, "aborting peer session while suspending trust");
            entry.task.abort();
            let _ = (&mut entry.task).await;
        }

        // The actor publishes all semantic updates before its terminal marker.
        // Preserve that ordering even though this service command owns removal.
        for update in drain_pending_session_updates(&mut self.update_rx) {
            match update {
                SessionUpdate::Event { generation: update_generation, event }
                    if update_generation == generation && event.session_id() == session_id =>
                {
                    self.emit(event).await;
                }
                update => self.handle_update(update).await,
            }
        }

        let mut close_reason = SessionCloseReason::TrustRevoked;
        let mut other_terminals = Vec::new();
        while let Ok(terminal) = self.terminal_rx.try_recv() {
            if terminal.generation == generation && terminal.session_id == session_id {
                close_reason = terminal.reason;
            } else {
                other_terminals.push(terminal);
            }
        }
        for terminal in other_terminals {
            self.handle_terminal(terminal).await;
        }

        self.publish_snapshot();
        self.emit(PeerSessionEvent::Closed {
            session_id,
            peer_id: peer_id.clone(),
            reason: close_reason,
        })
        .await;
    }

    fn connected_entry(&self, session_id: SessionId) -> Result<&SessionEntry, PeerSessionError> {
        let entry =
            self.sessions.get(&session_id).ok_or(PeerSessionError::SessionNotFound(session_id))?;
        let snapshot = entry.handle.snapshot();
        if snapshot.phase != PeerSessionPhase::Connected {
            return Err(PeerSessionError::InvalidState {
                session_id,
                phase: snapshot.phase.name(),
                operation: "access session media",
            });
        }
        Ok(entry)
    }

    fn publish_snapshot(&self) {
        let mut sessions =
            self.sessions.values().map(|entry| entry.handle.snapshot()).collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.session_id);
        let next = PeerSessionServiceSnapshot { sessions: Arc::new(sessions) };
        if *self.snapshot_tx.borrow() != next {
            self.snapshot_tx.send_replace(next);
        }
    }

    async fn shutdown_sessions(&mut self, deadline: TokioInstant) -> Result<(), PeerSessionError> {
        let active_generations = self
            .sessions
            .iter()
            .map(|(session_id, entry)| (*session_id, entry.handle.generation))
            .collect::<HashMap<_, _>>();
        let sessions = std::mem::take(&mut self.sessions);
        self.peers.clear();
        let actor_deadline = child_shutdown_deadline(deadline, self.limits.shutdown_timeout);
        for entry in sessions.values() {
            entry.handle.shutdown(actor_deadline);
        }
        let mut timed_out = false;
        for (_, mut entry) in sessions {
            if tokio::time::timeout_at(actor_deadline, &mut entry.task).await.is_err() {
                entry.task.abort();
                let _ = (&mut entry.task).await;
                timed_out = true;
            }
        }
        while let Ok(SessionUpdate::Event { generation, event }) = self.update_rx.try_recv() {
            if active_generations.get(&event.session_id()) == Some(&generation) {
                self.emit(event).await;
            }
        }
        while let Ok(terminal) = self.terminal_rx.try_recv() {
            if active_generations.get(&terminal.session_id) == Some(&terminal.generation) {
                self.emit(PeerSessionEvent::Closed {
                    session_id: terminal.session_id,
                    peer_id: terminal.peer_id,
                    reason: terminal.reason,
                })
                .await;
            }
        }
        self.publish_snapshot();
        if timed_out { Err(PeerSessionError::ShutdownTimeout) } else { Ok(()) }
    }
}

struct RecentSessionIds {
    entries: HashMap<SessionId, Instant>,
    retention: Duration,
    capacity: usize,
}

impl RecentSessionIds {
    fn new(retention: Duration, capacity: usize) -> Self {
        Self { entries: HashMap::new(), retention, capacity: capacity.max(1) }
    }

    fn seen_or_remember(&mut self, session_id: SessionId, now: Instant) -> bool {
        self.entries.retain(|_, seen_at| now.duration_since(*seen_at) <= self.retention);
        if self.entries.contains_key(&session_id) {
            return true;
        }
        if self.entries.len() >= self.capacity
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, seen_at)| **seen_at)
                .map(|(session_id, _)| *session_id)
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(session_id, now);
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncomingRequestResolution {
    Prompt,
    KeepOutgoing,
    ReplaceAndAccept,
    RejectExistingSession,
}

fn resolve_incoming_request(
    local_peer_id: &PeerId,
    remote_peer_id: &PeerId,
    existing_phase: Option<PeerSessionPhase>,
) -> IncomingRequestResolution {
    match existing_phase {
        None => IncomingRequestResolution::Prompt,
        Some(PeerSessionPhase::Requesting) if local_peer_id < remote_peer_id => {
            IncomingRequestResolution::KeepOutgoing
        }
        Some(PeerSessionPhase::Requesting) => IncomingRequestResolution::ReplaceAndAccept,
        Some(_) => IncomingRequestResolution::RejectExistingSession,
    }
}

fn discard_restart_connection(connection: NegotiationConnection, reason: &str) {
    tracing::debug!(?connection, reason, "discarding invalid ICE restart signaling");
    drop(connection);
}

async fn reject_connection(connection: NegotiationConnection, reason: &str, timeout: Duration) {
    let deadline = TokioInstant::now() + timeout;
    let _ = tokio::time::timeout_at(
        deadline,
        connection.send(super::protocol::NegotiationSignal::Reject { reason: reason.to_owned() }),
    )
    .await;
    connection.shutdown_until(deadline).await;
}

fn reply_service_error(command: ServiceCommand, error: PeerSessionError) {
    match command {
        ServiceCommand::Connect { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ServiceCommand::EnsureTrustSuspended { reply, .. }
        | ServiceCommand::ReleaseTrustSuspension { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ServiceCommand::Session { command, .. } => command.reply_error(error),
        ServiceCommand::EncodedVideoSink { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        ServiceCommand::RemoteVideoSource { reply, .. } => {
            let _ = reply.send(Err(error));
        }
    }
}

async fn receive_shutdown_deadline(
    shutdown_rx: &mut watch::Receiver<Option<TokioInstant>>,
) -> TokioInstant {
    loop {
        if let Some(deadline) = *shutdown_rx.borrow() {
            return deadline;
        }
        if shutdown_rx.changed().await.is_err() {
            return TokioInstant::now();
        }
    }
}

async fn wait_for_mandatory_sink_closed(sink: Option<mpsc::Sender<PeerSessionEvent>>) {
    match sink {
        Some(sink) => sink.closed().await,
        None => std::future::pending().await,
    }
}

fn drain_pending_session_updates(
    update_rx: &mut mpsc::Receiver<SessionUpdate>,
) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();
    while let Ok(update) = update_rx.try_recv() {
        updates.push(update);
    }
    updates
}

fn child_shutdown_deadline(
    owner_deadline: TokioInstant,
    shutdown_timeout: Duration,
) -> TokioInstant {
    let cleanup_grace = shutdown_timeout
        .checked_div(10)
        .unwrap_or_default()
        .clamp(Duration::from_millis(100), Duration::from_millis(500));
    owner_deadline.checked_sub(cleanup_grace).unwrap_or(owner_deadline)
}

fn negotiation_limits(limits: &PeerSessionLimits) -> Result<NegotiationLimits, PeerSessionError> {
    if limits.ice_restart_timeout.is_zero() {
        return Err(PeerSessionError::InvalidLimit { name: "ice_restart_timeout" });
    }
    if limits.signaling_auth_global_burst == 0 {
        return Err(PeerSessionError::InvalidLimit { name: "signaling_auth_global_burst" });
    }
    if limits.signaling_auth_global_refill_interval.is_zero() {
        return Err(PeerSessionError::InvalidLimit {
            name: "signaling_auth_global_refill_interval",
        });
    }
    if limits.signaling_auth_per_ip_burst == 0 {
        return Err(PeerSessionError::InvalidLimit { name: "signaling_auth_per_ip_burst" });
    }
    if limits.signaling_auth_per_ip_refill_interval.is_zero() {
        return Err(PeerSessionError::InvalidLimit {
            name: "signaling_auth_per_ip_refill_interval",
        });
    }
    if limits.max_signaling_auth_tracked_ips == 0 {
        return Err(PeerSessionError::InvalidLimit { name: "max_signaling_auth_tracked_ips" });
    }
    let max_message_age = ChronoDuration::from_std(limits.signaling_max_message_age)
        .map_err(|_| PeerSessionError::Protocol("invalid signaling max age".into()))?;
    let max_clock_skew = ChronoDuration::from_std(limits.signaling_max_clock_skew)
        .map_err(|_| PeerSessionError::Protocol("invalid signaling clock skew".into()))?;
    Ok(NegotiationLimits {
        max_frame_bytes: limits.max_signaling_frame_bytes.max(1024),
        queue_capacity: limits.signaling_queue_capacity.max(1),
        replay_capacity: limits.signaling_replay_capacity.max(1),
        max_connections: limits.max_signaling_connections.max(1),
        max_connections_per_ip: limits.max_signaling_connections_per_ip.max(1),
        authentication_global_burst: limits.signaling_auth_global_burst,
        authentication_global_refill_interval: limits.signaling_auth_global_refill_interval,
        authentication_per_ip_burst: limits.signaling_auth_per_ip_burst,
        authentication_per_ip_refill_interval: limits.signaling_auth_per_ip_refill_interval,
        max_authentication_tracked_ips: limits.max_signaling_auth_tracked_ips,
        handshake_timeout: limits.signaling_handshake_timeout,
        idle_timeout: limits.signaling_idle_timeout,
        max_message_age,
        max_clock_skew,
    })
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    #[derive(Debug, Default)]
    struct TestDirectory {
        trusted: std::sync::RwLock<HashMap<PeerId, TrustedPeerIdentity>>,
        endpoint_hints: std::sync::RwLock<HashMap<PeerId, Arc<[SocketAddr]>>>,
    }

    impl TestDirectory {
        fn insert_peer(&self, peer_id: PeerId, public_key: String, endpoint: SocketAddr) {
            self.insert_peer_with_hints(peer_id, public_key, Arc::from([endpoint]));
        }

        fn insert_peer_with_hints(
            &self,
            peer_id: PeerId,
            public_key: String,
            endpoint_hints: Arc<[SocketAddr]>,
        ) {
            self.trusted
                .write()
                .unwrap()
                .insert(peer_id.clone(), TrustedPeerIdentity::new(peer_id.clone(), public_key));
            self.endpoint_hints.write().unwrap().insert(peer_id, endpoint_hints);
        }
    }

    #[async_trait]
    impl TrustedPeerResolver for TestDirectory {
        async fn trusted_peer(
            &self,
            peer_id: &PeerId,
        ) -> Result<Option<TrustedPeerIdentity>, PeerSessionError> {
            Ok(self.trusted.read().unwrap().get(peer_id).cloned())
        }
    }

    #[async_trait]
    impl PeerEndpointResolver for TestDirectory {
        async fn endpoint_hints_for(
            &self,
            peer_id: &PeerId,
        ) -> Result<Arc<[SocketAddr]>, PeerSessionError> {
            Ok(self
                .endpoint_hints
                .read()
                .unwrap()
                .get(peer_id)
                .cloned()
                .unwrap_or_else(|| Arc::from([])))
        }
    }

    #[derive(Debug, Default)]
    struct BlockingDirectory {
        entered: tokio::sync::Notify,
    }

    #[async_trait]
    impl TrustedPeerResolver for BlockingDirectory {
        async fn trusted_peer(
            &self,
            _peer_id: &PeerId,
        ) -> Result<Option<TrustedPeerIdentity>, PeerSessionError> {
            self.entered.notify_one();
            std::future::pending().await
        }
    }

    #[async_trait]
    impl PeerEndpointResolver for BlockingDirectory {
        async fn endpoint_hints_for(
            &self,
            _peer_id: &PeerId,
        ) -> Result<Arc<[SocketAddr]>, PeerSessionError> {
            Ok(Arc::from([]))
        }
    }

    #[test]
    fn rejects_disabled_signaling_authentication_limits() {
        macro_rules! assert_invalid_limit {
            ($field:ident, $value:expr) => {{
                let mut limits = PeerSessionLimits::default();
                limits.$field = $value;
                assert!(matches!(
                    negotiation_limits(&limits),
                    Err(PeerSessionError::InvalidLimit { name })
                        if name == stringify!($field)
                ));
            }};
        }

        assert_invalid_limit!(signaling_auth_global_burst, 0);
        assert_invalid_limit!(signaling_auth_global_refill_interval, Duration::ZERO);
        assert_invalid_limit!(signaling_auth_per_ip_burst, 0);
        assert_invalid_limit!(signaling_auth_per_ip_refill_interval, Duration::ZERO);
        assert_invalid_limit!(max_signaling_auth_tracked_ips, 0);
        assert_invalid_limit!(ice_restart_timeout, Duration::ZERO);
    }

    async fn start_test_pair() -> (PeerSessionService, PeerSessionService, PeerId, PeerId) {
        let peer_a = PeerId::new("peer-a").unwrap();
        let peer_b = PeerId::new("peer-b").unwrap();
        let directory_a = Arc::new(TestDirectory::default());
        let directory_b = Arc::new(TestDirectory::default());
        let mut config_a = PeerSessionServiceConfig::new(directory_a.clone(), directory_a.clone());
        config_a.local_peer_id = Some(peer_a.clone());
        config_a.limits.request_timeout = Duration::from_secs(5);
        config_a.limits.negotiation_timeout = Duration::from_secs(10);
        let mut config_b = PeerSessionServiceConfig::new(directory_b.clone(), directory_b.clone());
        config_b.local_peer_id = Some(peer_b.clone());
        config_b.limits.request_timeout = Duration::from_secs(5);
        config_b.limits.negotiation_timeout = Duration::from_secs(10);

        let service_a = PeerSessionService::start(config_a).await.unwrap();
        let service_b = PeerSessionService::start(config_b).await.unwrap();
        directory_a.insert_peer(
            peer_b.clone(),
            service_b.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
        );
        directory_b.insert_peer(
            peer_a.clone(),
            service_a.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
        );
        (service_a, service_b, peer_a, peer_b)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn connect_falls_back_after_a_wrong_peer_endpoint_fails_authentication() {
        let peer_a = PeerId::new("peer-a").unwrap();
        let peer_b = PeerId::new("peer-b").unwrap();
        let peer_c = PeerId::new("peer-c").unwrap();
        let directory_a = Arc::new(TestDirectory::default());
        let directory_b = Arc::new(TestDirectory::default());
        let directory_c = Arc::new(TestDirectory::default());
        let mut config_a = PeerSessionServiceConfig::new(directory_a.clone(), directory_a.clone());
        config_a.local_peer_id = Some(peer_a.clone());
        config_a.limits.endpoint_attempt_timeout = Duration::from_millis(250);
        let mut config_b = PeerSessionServiceConfig::new(directory_b.clone(), directory_b.clone());
        config_b.local_peer_id = Some(peer_b.clone());
        let mut config_c = PeerSessionServiceConfig::new(directory_c.clone(), directory_c.clone());
        config_c.local_peer_id = Some(peer_c);

        let service_a = PeerSessionService::start(config_a).await.unwrap();
        let service_b = PeerSessionService::start(config_b).await.unwrap();
        let service_c = PeerSessionService::start(config_c).await.unwrap();
        directory_a.insert_peer_with_hints(
            peer_b.clone(),
            service_b.local_public_key(),
            Arc::from([
                SocketAddr::from(([127, 0, 0, 1], service_c.signaling_port())),
                SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
            ]),
        );
        directory_b.insert_peer(
            peer_a.clone(),
            service_a.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
        );
        directory_c.insert_peer(
            peer_a.clone(),
            service_a.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
        );

        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_b = handle_b.subscribe_events();
        let mut events_c = service_c.handle().subscribe_events();
        let session_id = handle_a.connect(peer_b).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        assert!(tokio::time::timeout(Duration::from_millis(100), events_c.recv()).await.is_err());

        tokio::time::timeout(Duration::from_secs(6), async {
            let (result_a, result_b, result_c) =
                tokio::join!(service_a.shutdown(), service_b.shutdown(), service_c.shutdown());
            result_a.unwrap();
            result_b.unwrap();
            result_c.unwrap();
        })
        .await
        .expect("fallback test services did not shut down within their owner deadline");
    }

    #[tokio::test]
    async fn exhausted_endpoint_hints_return_a_structured_error_without_a_session() {
        let local_peer = PeerId::new("local").unwrap();
        let remote_peer = PeerId::new("remote").unwrap();
        let directory = Arc::new(TestDirectory::default());
        let mut config = PeerSessionServiceConfig::new(directory.clone(), directory.clone());
        config.local_peer_id = Some(local_peer);
        config.limits.max_endpoint_attempts = 1;
        config.limits.endpoint_attempt_timeout = Duration::from_millis(500);
        let service = PeerSessionService::start(config).await.unwrap();
        let failing_listener =
            tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let failing_endpoint = failing_listener.local_addr().unwrap();
        let failing_task = tokio::spawn(async move {
            let (stream, _) = failing_listener.accept().await.unwrap();
            drop(stream);
        });
        directory.insert_peer_with_hints(
            remote_peer.clone(),
            LocalPeerIdentity::generate().public_key_base64(),
            Arc::from([failing_endpoint, SocketAddr::from(([127, 0, 0, 1], 1))]),
        );

        assert_eq!(
            service.handle().connect(remote_peer.clone()).await,
            Err(PeerSessionError::EndpointAttemptsExhausted { peer_id: remote_peer, attempted: 1 })
        );
        failing_task.await.unwrap();
        assert!(service.handle().snapshot().sessions.is_empty());
        service.shutdown().await.unwrap();
    }

    #[test]
    fn simultaneous_connect_converges_on_lower_peer_id_as_offerer() {
        let lower = PeerId::new("a").unwrap();
        let higher = PeerId::new("b").unwrap();

        assert_eq!(
            resolve_incoming_request(&lower, &higher, Some(PeerSessionPhase::Requesting)),
            IncomingRequestResolution::KeepOutgoing
        );
        assert_eq!(
            resolve_incoming_request(&higher, &lower, Some(PeerSessionPhase::Requesting)),
            IncomingRequestResolution::ReplaceAndAccept
        );
    }

    #[test]
    fn incoming_request_never_evicts_a_non_requesting_session() {
        let local = PeerId::new("a").unwrap();
        let remote = PeerId::new("b").unwrap();
        for phase in [
            PeerSessionPhase::Incoming,
            PeerSessionPhase::Negotiating,
            PeerSessionPhase::Connected,
            PeerSessionPhase::Disconnecting,
        ] {
            assert_eq!(
                resolve_incoming_request(&local, &remote, Some(phase)),
                IncomingRequestResolution::RejectExistingSession,
                "phase={phase:?}"
            );
        }
    }

    #[test]
    fn recent_session_identifiers_are_capacity_bounded() {
        let now = Instant::now();
        let mut recent = RecentSessionIds::new(Duration::from_secs(60), 2);
        let first = SessionId::new();
        let second = SessionId::new();
        let third = SessionId::new();
        assert!(!recent.seen_or_remember(first, now));
        assert!(!recent.seen_or_remember(second, now + Duration::from_millis(1)));
        assert!(!recent.seen_or_remember(third, now + Duration::from_millis(2)));
        assert_eq!(recent.entries.len(), 2);
        assert!(!recent.entries.contains_key(&first));
    }

    #[tokio::test]
    async fn terminal_overtake_drains_semantic_updates_before_closed() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer").unwrap();
        let generation = uuid::Uuid::new_v4();
        let message = PeerSessionEvent::MessageReceived {
            session_id,
            peer_id: peer_id.clone(),
            message_id: MessageId::new(),
            body: "accepted before terminal".into(),
            sent_at: Utc::now(),
        };
        let (update_tx, mut update_rx) = mpsc::channel(2);
        update_tx.send(SessionUpdate::Event { generation, event: message.clone() }).await.unwrap();
        // Model the unbounded terminal channel winning select before update_rx.
        let terminal = PeerSessionEvent::Closed {
            session_id,
            peer_id,
            reason: super::super::SessionCloseReason::RemoteDisconnect,
        };
        let (sink_tx, mut sink_rx) = mpsc::channel(2);
        for update in drain_pending_session_updates(&mut update_rx) {
            let SessionUpdate::Event { event, .. } = update;
            sink_tx.try_send(event).unwrap();
        }
        sink_tx.try_send(terminal.clone()).unwrap();

        assert_eq!(sink_rx.recv().await.unwrap(), message);
        assert_eq!(sink_rx.recv().await.unwrap(), terminal);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_connect_converges_and_active_shutdown_joins_both_services() {
        let (service_a, service_b, peer_a, peer_b) = start_test_pair().await;
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();

        let (outgoing_a, outgoing_b) =
            tokio::join!(handle_a.connect(peer_b.clone()), handle_b.connect(peer_a.clone()),);
        let winning_session = outgoing_a.unwrap();
        let _superseded_session = outgoing_b.unwrap();
        wait_for_connected(&mut events_a, winning_session).await;
        wait_for_connected(&mut events_b, winning_session).await;
        assert_eq!(handle_a.snapshot().sessions.len(), 1);
        assert_eq!(handle_b.snapshot().sessions.len(), 1);
        assert_eq!(handle_a.snapshot().sessions[0].session_id, winning_session);
        assert_eq!(handle_b.snapshot().sessions[0].session_id, winning_session);

        tokio::time::timeout(Duration::from_secs(6), async {
            let (result_a, result_b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            result_a.unwrap();
            result_b.unwrap();
        })
        .await
        .expect("active peer services did not shut down within their owner deadline");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn negotiating_sessions_shutdown_with_one_owner_deadline() {
        let (service_a, service_b, peer_a, peer_b) = start_test_pair().await;
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_b = handle_b.subscribe_events();
        let session_id = handle_a.connect(peer_b).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);

        tokio::time::timeout(Duration::from_secs(6), async {
            let (result_a, result_b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            result_a.unwrap();
            result_b.unwrap();
        })
        .await
        .expect("negotiating peer services did not shut down within their owner deadline");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn suspending_trust_closes_the_session_blocks_connect_and_can_be_resumed() {
        let (service_a, service_b, peer_a, peer_b) = start_test_pair().await;
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();
        let barrier_owner = TrustBarrierOwnerId::allocate();

        let session_id = handle_a.connect(peer_b.clone()).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        handle_b.accept(session_id).await.unwrap();
        wait_for_connected(&mut events_a, session_id).await;
        wait_for_connected(&mut events_b, session_id).await;

        handle_a.ensure_trust_suspended(peer_b.clone(), barrier_owner).await.unwrap();
        let closed = wait_for_event(&mut events_a, |event| {
            matches!(
                event,
                PeerSessionEvent::Closed {
                    session_id: id,
                    reason: SessionCloseReason::TrustRevoked,
                    ..
                } if *id == session_id
            )
        })
        .await;
        assert!(matches!(
            closed,
            PeerSessionEvent::Closed { reason: SessionCloseReason::TrustRevoked, .. }
        ));
        assert!(handle_a.snapshot().session_for_peer(&peer_b).is_none());
        assert_eq!(
            handle_a.connect(peer_b.clone()).await,
            Err(PeerSessionError::PeerSuspended(peer_b.clone()))
        );
        wait_for_closed(&mut events_b, session_id).await;
        wait_for_absent(&handle_b, session_id).await;

        handle_a.ensure_trust_suspended(peer_b.clone(), barrier_owner).await.unwrap();
        handle_a.release_trust_suspension(peer_b.clone(), barrier_owner).await.unwrap();
        handle_a.release_trust_suspension(peer_b.clone(), barrier_owner).await.unwrap();
        let reconnected = handle_a.connect(peer_b).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, reconnected);
        handle_b.accept(reconnected).await.unwrap();
        wait_for_connected(&mut events_a, reconnected).await;
        wait_for_connected(&mut events_b, reconnected).await;

        tokio::time::timeout(Duration::from_secs(6), async {
            let (result_a, result_b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            result_a.unwrap();
            result_b.unwrap();
        })
        .await
        .expect("resumed peer services did not shut down within their owner deadline");
    }

    #[tokio::test]
    async fn independent_trust_barrier_owners_release_independently() {
        let directory = Arc::new(TestDirectory::default());
        let mut config = PeerSessionServiceConfig::new(directory.clone(), directory);
        config.local_peer_id = Some(PeerId::new("local").unwrap());
        let service = PeerSessionService::start(config).await.unwrap();
        let handle = service.handle();
        let remote_peer = PeerId::new("remote").unwrap();
        let owner_a = TrustBarrierOwnerId::allocate();
        let owner_b = TrustBarrierOwnerId::allocate();

        handle.ensure_trust_suspended(remote_peer.clone(), owner_a).await.unwrap();
        handle.ensure_trust_suspended(remote_peer.clone(), owner_b).await.unwrap();
        handle.release_trust_suspension(remote_peer.clone(), owner_a).await.unwrap();
        assert_eq!(
            handle.connect(remote_peer.clone()).await,
            Err(PeerSessionError::PeerSuspended(remote_peer.clone()))
        );

        handle.release_trust_suspension(remote_peer.clone(), owner_b).await.unwrap();
        assert_eq!(
            handle.connect(remote_peer.clone()).await,
            Err(PeerSessionError::PeerNotTrusted(remote_peer))
        );
        service.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn admitted_commands_queued_behind_connect_get_service_stopped_on_shutdown() {
        let directory = Arc::new(BlockingDirectory::default());
        let mut config = PeerSessionServiceConfig::new(directory.clone(), directory.clone());
        config.local_peer_id = Some(PeerId::new("local").unwrap());
        let service = PeerSessionService::start(config).await.unwrap();
        let handle = service.handle();
        let first_handle = handle.clone();
        let first =
            tokio::spawn(async move { first_handle.connect(PeerId::new("first").unwrap()).await });
        directory.entered.notified().await;

        let (queued_reply_tx, queued_reply_rx) = oneshot::channel();
        handle
            .command_tx
            .send(ServiceCommand::Connect {
                peer_id: PeerId::new("queued").unwrap(),
                reply: queued_reply_tx,
            })
            .await
            .unwrap();

        service.shutdown().await.unwrap();
        assert_eq!(first.await.unwrap(), Err(PeerSessionError::ServiceStopped));
        assert_eq!(queued_reply_rx.await.unwrap(), Err(PeerSessionError::ServiceStopped));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn mandatory_event_sink_overflow_fails_closed_and_blocks_new_sessions() {
        let peer_a = PeerId::new("peer-a").unwrap();
        let peer_b = PeerId::new("peer-b").unwrap();
        let directory_a = Arc::new(TestDirectory::default());
        let directory_b = Arc::new(TestDirectory::default());
        let mut config_a = PeerSessionServiceConfig::new(directory_a.clone(), directory_a.clone());
        config_a.local_peer_id = Some(peer_a.clone());
        let mut config_b = PeerSessionServiceConfig::new(directory_b.clone(), directory_b.clone());
        config_b.local_peer_id = Some(peer_b.clone());
        let (sink_tx, _sink_rx) = mpsc::channel(1);
        config_b.mandatory_event_sink = Some(sink_tx);

        let service_a = PeerSessionService::start(config_a).await.unwrap();
        let service_b = PeerSessionService::start(config_b).await.unwrap();
        directory_a.insert_peer(
            peer_b.clone(),
            service_b.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
        );
        directory_b.insert_peer(
            peer_a.clone(),
            service_a.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
        );
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();

        let session_id = handle_a.connect(peer_b.clone()).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        handle_b.accept(session_id).await.unwrap();
        wait_for_closed(&mut events_a, session_id).await;
        wait_for_closed(&mut events_b, session_id).await;
        wait_for_absent(&handle_a, session_id).await;
        wait_for_absent(&handle_b, session_id).await;

        let retry_id = handle_a.connect(peer_b).await.unwrap();
        wait_for_closed(&mut events_a, retry_id).await;
        assert!(handle_b.snapshot().sessions.is_empty());

        tokio::time::timeout(Duration::from_secs(6), async {
            let (result_a, result_b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            result_a.unwrap();
            result_b.unwrap();
        })
        .await
        .expect("services did not shut down after mandatory sink failure");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn mandatory_event_sink_closure_proactively_terminates_an_idle_session() {
        let peer_a = PeerId::new("peer-a").unwrap();
        let peer_b = PeerId::new("peer-b").unwrap();
        let directory_a = Arc::new(TestDirectory::default());
        let directory_b = Arc::new(TestDirectory::default());
        let mut config_a = PeerSessionServiceConfig::new(directory_a.clone(), directory_a.clone());
        config_a.local_peer_id = Some(peer_a.clone());
        let mut config_b = PeerSessionServiceConfig::new(directory_b.clone(), directory_b.clone());
        config_b.local_peer_id = Some(peer_b.clone());
        let (sink_tx, sink_rx) = mpsc::channel(16);
        config_b.mandatory_event_sink = Some(sink_tx);

        let service_a = PeerSessionService::start(config_a).await.unwrap();
        let service_b = PeerSessionService::start(config_b).await.unwrap();
        directory_a.insert_peer(
            peer_b.clone(),
            service_b.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
        );
        directory_b.insert_peer(
            peer_a.clone(),
            service_a.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
        );
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();
        let session_id = handle_a.connect(peer_b).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        handle_b.accept(session_id).await.unwrap();
        wait_for_connected(&mut events_a, session_id).await;
        wait_for_connected(&mut events_b, session_id).await;

        drop(sink_rx);
        wait_for_closed(&mut events_a, session_id).await;
        wait_for_closed(&mut events_b, session_id).await;
        wait_for_absent(&handle_a, session_id).await;
        wait_for_absent(&handle_b, session_id).await;

        tokio::time::timeout(Duration::from_secs(6), async {
            let (result_a, result_b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            result_a.unwrap();
            result_b.unwrap();
        })
        .await
        .expect("services did not shut down after mandatory sink closure");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_peer_loopback_covers_reject_message_receipt_share_and_reconnect() {
        let peer_a = PeerId::new("peer-a").unwrap();
        let peer_b = PeerId::new("peer-b").unwrap();
        let directory_a = Arc::new(TestDirectory::default());
        let directory_b = Arc::new(TestDirectory::default());
        let mut config_a = PeerSessionServiceConfig::new(directory_a.clone(), directory_a.clone());
        config_a.local_peer_id = Some(peer_a.clone());
        config_a.limits.request_timeout = Duration::from_secs(5);
        config_a.limits.negotiation_timeout = Duration::from_secs(10);
        let mut config_b = PeerSessionServiceConfig::new(directory_b.clone(), directory_b.clone());
        config_b.local_peer_id = Some(peer_b.clone());
        config_b.limits.request_timeout = Duration::from_secs(5);
        config_b.limits.negotiation_timeout = Duration::from_secs(10);

        let service_a = PeerSessionService::start(config_a).await.unwrap();
        let service_b = PeerSessionService::start(config_b).await.unwrap();
        directory_a.insert_peer(
            peer_b.clone(),
            service_b.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
        );
        directory_b.insert_peer(
            peer_a.clone(),
            service_a.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
        );
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();

        // First prove a rejected request is terminal and leaves both registries reusable.
        let rejected_id = handle_a.connect(peer_b.clone()).await.unwrap();
        let incoming_id = wait_for_incoming(&mut events_b, &peer_a).await;
        assert_eq!(incoming_id, rejected_id);
        handle_b.reject(incoming_id, "not now").await.unwrap();
        wait_for_closed(&mut events_a, rejected_id).await;
        wait_for_closed(&mut events_b, incoming_id).await;
        wait_for_absent(&handle_a, rejected_id).await;
        wait_for_absent(&handle_b, incoming_id).await;

        // Reconnect on the same long-lived services, accept, and exercise session capabilities.
        let session_id = handle_a.connect(peer_b.clone()).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        handle_b.accept(session_id).await.unwrap();
        wait_for_connected(&mut events_a, session_id).await;
        wait_for_connected(&mut events_b, session_id).await;

        let message_id = MessageId::new();
        let sent_at = Utc::now();
        handle_a.send_message(session_id, message_id, "hello", sent_at).await.unwrap();
        let received = wait_for_event(&mut events_b, |event| {
            matches!(
                event,
                PeerSessionEvent::MessageReceived {
                    session_id: id,
                    message_id: received_id,
                    body,
                    ..
                } if *id == session_id && *received_id == message_id && body == "hello"
            )
        })
        .await;
        assert!(matches!(received, PeerSessionEvent::MessageReceived { .. }));

        let received_at = Utc::now();
        handle_b.send_receipt(session_id, message_id, received_at).await.unwrap();
        wait_for_event(&mut events_a, |event| {
            matches!(
                event,
                PeerSessionEvent::MessageReceiptReceived {
                    session_id: id,
                    message_id: received_id,
                    ..
                } if *id == session_id && *received_id == message_id
            )
        })
        .await;

        let share_id = handle_a.start_screen_share(session_id).await.unwrap();
        wait_for_event(&mut events_b, |event| {
            matches!(
                event,
                PeerSessionEvent::RemoteShareChanged {
                    session_id: id,
                    state: super::super::RemoteShareState::Active { share_id: remote_id, .. },
                    ..
                } if *id == session_id && *remote_id == share_id
            )
        })
        .await;
        handle_a.stop_screen_share(session_id, share_id).await.unwrap();
        wait_for_event(&mut events_b, |event| {
            matches!(
                event,
                PeerSessionEvent::RemoteShareChanged {
                    session_id: id,
                    state: super::super::RemoteShareState::Inactive,
                    ..
                } if *id == session_id
            )
        })
        .await;

        handle_a.disconnect(session_id).await.unwrap();
        wait_for_closed(&mut events_a, session_id).await;
        wait_for_closed(&mut events_b, session_id).await;
        wait_for_absent(&handle_a, session_id).await;
        wait_for_absent(&handle_b, session_id).await;

        tokio::time::timeout(Duration::from_secs(5), service_a.shutdown())
            .await
            .expect("service A shutdown timed out")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), service_b.shutdown())
            .await
            .expect("service B shutdown timed out")
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn rtp_share_epoch_survives_fragmentation_and_rejects_a_stale_sink() {
        fn h264_sample(marker: u8, size: usize) -> super::super::EncodedVideoSample {
            let mut data = vec![marker; size.max(3)];
            data[0] = 0x65;
            super::super::EncodedVideoSample::new(data, Duration::from_millis(16))
        }

        async fn receive_marker(
            source: &mut RemoteVideoSource,
            epoch: super::super::ShareEpoch,
        ) -> u8 {
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    match source.recv_for(epoch).await.unwrap() {
                        super::super::RemoteVideoRead::Sample(sample) => {
                            if let Some(marker) = sample.data.get(5) {
                                return *marker;
                            }
                        }
                        super::super::RemoteVideoRead::EpochAdvanced { next_epoch } => {
                            panic!(
                                "media advanced to epoch {} while waiting for {}",
                                next_epoch.value(),
                                epoch.value()
                            );
                        }
                    }
                }
            })
            .await
            .expect("timed out waiting for epoch-tagged video")
        }

        let (service_a, service_b, peer_a, peer_b) = start_test_pair().await;
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();

        let session_id = handle_a.connect(peer_b).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        handle_b.accept(session_id).await.unwrap();
        wait_for_connected(&mut events_a, session_id).await;
        wait_for_connected(&mut events_b, session_id).await;
        let mut remote = handle_b.subscribe_remote_video(session_id).await.unwrap();

        let share_a = handle_a.start_screen_share(session_id).await.unwrap();
        wait_for_event(&mut events_b, |event| {
            matches!(
                event,
                PeerSessionEvent::RemoteShareChanged {
                    session_id: id,
                    state: super::super::RemoteShareState::Active {
                        share_id,
                        epoch: super::super::ShareEpoch::FIRST,
                    },
                    ..
                } if *id == session_id && *share_id == share_a
            )
        })
        .await;
        let sink_a = handle_a.encoded_video_sink(session_id, share_a).await.unwrap();
        sink_a.send(h264_sample(0xa1, 5_000)).await.unwrap();
        sink_a.send(h264_sample(0xa2, 32)).await.unwrap();
        sink_a.send(h264_sample(0xa3, 32)).await.unwrap();
        assert_eq!(receive_marker(&mut remote, super::super::ShareEpoch::FIRST).await, 0xa1);

        handle_a.stop_screen_share(session_id, share_a).await.unwrap();
        wait_for_event(&mut events_b, |event| {
            matches!(
                event,
                PeerSessionEvent::RemoteShareChanged {
                    session_id: id,
                    state: super::super::RemoteShareState::Inactive,
                    ..
                } if *id == session_id
            )
        })
        .await;
        let share_b = handle_a.start_screen_share(session_id).await.unwrap();
        let epoch_b = super::super::ShareEpoch::FIRST.next().unwrap();
        wait_for_event(&mut events_b, |event| {
            matches!(
                event,
                PeerSessionEvent::RemoteShareChanged {
                    session_id: id,
                    state: super::super::RemoteShareState::Active { share_id, epoch },
                    ..
                } if *id == session_id && *share_id == share_b && *epoch == epoch_b
            )
        })
        .await;

        // The old producer can still race cancellation, but its immutable A
        // capability is revoked and cannot backpressure or be relabelled as B.
        assert_eq!(sink_a.send(h264_sample(0xaf, 5_000)).await, Err(PeerSessionError::MediaClosed));
        let sink_b = handle_a.encoded_video_sink(session_id, share_b).await.unwrap();
        sink_b.send(h264_sample(0xb1, 5_000)).await.unwrap();
        sink_b.send(h264_sample(0xb2, 32)).await.unwrap();
        sink_b.send(h264_sample(0xb3, 32)).await.unwrap();
        assert_eq!(receive_marker(&mut remote, epoch_b).await, 0xb1);

        handle_a.disconnect(session_id).await.unwrap();
        wait_for_closed(&mut events_a, session_id).await;
        wait_for_closed(&mut events_b, session_id).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            let (a, b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            a.unwrap();
            b.unwrap();
        })
        .await
        .expect("media epoch test services did not shut down");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn ice_restart_preserves_session_channels_and_active_share_state() {
        let (service_a, service_b, peer_a, peer_b) = start_test_pair().await;
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();

        let session_id = handle_a.connect(peer_b).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        handle_b.accept(session_id).await.unwrap();
        wait_for_connected(&mut events_a, session_id).await;
        wait_for_connected(&mut events_b, session_id).await;

        let share_id = handle_a.start_screen_share(session_id).await.unwrap();
        wait_for_event(&mut events_b, |event| {
            matches!(
                event,
                PeerSessionEvent::RemoteShareChanged {
                    session_id: id,
                    state: super::super::RemoteShareState::Active { share_id: remote_id, .. },
                    ..
                } if *id == session_id && *remote_id == share_id
            )
        })
        .await;

        // Let the initial DTLS/SCTP association leave its just-open callback
        // window before forcing a healthy-path restart in this test. Production
        // recovery is triggered only after the configured ICE disconnect grace.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let generation_a = handle_a.committed_transport_generation(session_id).await.unwrap();
        let generation_b = handle_b.committed_transport_generation(session_id).await.unwrap();
        handle_a.force_ice_restart(session_id).await.unwrap();
        let ((), ()) = tokio::join!(
            wait_for_transport_generation(&handle_a, session_id, generation_a),
            wait_for_transport_generation(&handle_b, session_id, generation_b),
        );

        let snapshot_a = handle_a.snapshot();
        let snapshot_b = handle_b.snapshot();
        let session_a = snapshot_a.session(session_id).expect("offerer session survived restart");
        let session_b = snapshot_b.session(session_id).expect("answerer session survived restart");
        assert_eq!(
            session_a.local_share,
            super::super::LocalShareState::Active {
                share_id,
                epoch: super::super::ShareEpoch::FIRST,
            }
        );
        assert_eq!(
            session_b.remote_share,
            super::super::RemoteShareState::Active {
                share_id,
                epoch: super::super::ShareEpoch::FIRST,
            }
        );
        assert_eq!(snapshot_a.sessions.len(), 1);
        assert_eq!(snapshot_b.sessions.len(), 1);

        let message_id = MessageId::new();
        handle_a.send_message(session_id, message_id, "after restart", Utc::now()).await.unwrap();
        let post_restart_event = wait_for_event(&mut events_b, |event| {
            matches!(
                event,
                PeerSessionEvent::MessageReceived {
                    session_id: id,
                    message_id: received_id,
                    body,
                    ..
                } if *id == session_id && *received_id == message_id && body == "after restart"
            ) || matches!(
                event,
                PeerSessionEvent::Closed { session_id: id, .. } if *id == session_id
            )
        })
        .await;
        assert!(
            matches!(post_restart_event, PeerSessionEvent::MessageReceived { .. }),
            "session closed after restart instead of delivering data: {post_restart_event:?}"
        );

        handle_a.disconnect(session_id).await.unwrap();
        wait_for_closed(&mut events_a, session_id).await;
        wait_for_closed(&mut events_b, session_id).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            let (a, b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            a.unwrap();
            b.unwrap();
        })
        .await
        .expect("restart test services did not shut down");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn answerer_initiated_and_simultaneous_restarts_converge_without_glare() {
        let (service_a, service_b, peer_a, peer_b) = start_test_pair().await;
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();
        let session_id = handle_a.connect(peer_b).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        handle_b.accept(session_id).await.unwrap();
        wait_for_connected(&mut events_a, session_id).await;
        wait_for_connected(&mut events_b, session_id).await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let generation_a = handle_a.committed_transport_generation(session_id).await.unwrap();
        let generation_b = handle_b.committed_transport_generation(session_id).await.unwrap();
        let (forced, (), ()) = tokio::join!(
            handle_b.force_ice_restart(session_id),
            wait_for_transport_generation(&handle_a, session_id, generation_a),
            wait_for_transport_generation(&handle_b, session_id, generation_b),
        );
        forced.unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;
        let generation_a = handle_a.committed_transport_generation(session_id).await.unwrap();
        let generation_b = handle_b.committed_transport_generation(session_id).await.unwrap();
        let (forced_a, forced_b, (), ()) = tokio::join!(
            handle_a.force_ice_restart(session_id),
            handle_b.force_ice_restart(session_id),
            wait_for_transport_generation(&handle_a, session_id, generation_a),
            wait_for_transport_generation(&handle_b, session_id, generation_b),
        );
        forced_a.unwrap();
        forced_b.unwrap();

        assert_eq!(handle_a.snapshot().sessions.len(), 1);
        assert_eq!(handle_b.snapshot().sessions.len(), 1);
        let message_id = MessageId::new();
        handle_b
            .send_message(session_id, message_id, "after simultaneous restart", Utc::now())
            .await
            .unwrap();
        wait_for_event(&mut events_a, |event| {
            matches!(
                event,
                PeerSessionEvent::MessageReceived {
                    session_id: id,
                    message_id: received_id,
                    body,
                    ..
                } if *id == session_id
                    && *received_id == message_id
                    && body == "after simultaneous restart"
            )
        })
        .await;

        handle_a.disconnect(session_id).await.unwrap();
        wait_for_closed(&mut events_a, session_id).await;
        wait_for_closed(&mut events_b, session_id).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            let (a, b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            a.unwrap();
            b.unwrap();
        })
        .await
        .expect("simultaneous restart services did not shut down");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn unreachable_restart_signaling_is_removed_within_the_attempt_deadline() {
        let peer_a = PeerId::new("peer-a").unwrap();
        let peer_b = PeerId::new("peer-b").unwrap();
        let directory_a = Arc::new(TestDirectory::default());
        let directory_b = Arc::new(TestDirectory::default());
        let mut config_a = PeerSessionServiceConfig::new(directory_a.clone(), directory_a.clone());
        config_a.local_peer_id = Some(peer_a.clone());
        config_a.limits.ice_restart_timeout = Duration::from_millis(150);
        config_a.limits.endpoint_attempt_timeout = Duration::from_secs(5);
        config_a.limits.shutdown_timeout = Duration::from_millis(500);
        let mut config_b = PeerSessionServiceConfig::new(directory_b.clone(), directory_b.clone());
        config_b.local_peer_id = Some(peer_b.clone());
        config_b.limits.shutdown_timeout = Duration::from_millis(500);
        let service_a = PeerSessionService::start(config_a).await.unwrap();
        let service_b = PeerSessionService::start(config_b).await.unwrap();
        directory_a.insert_peer(
            peer_b.clone(),
            service_b.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
        );
        directory_b.insert_peer(
            peer_a.clone(),
            service_a.local_public_key(),
            SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
        );
        let handle_a = service_a.handle();
        let handle_b = service_b.handle();
        let mut events_a = handle_a.subscribe_events();
        let mut events_b = handle_b.subscribe_events();
        let session_id = handle_a.connect(peer_b.clone()).await.unwrap();
        assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
        handle_b.accept(session_id).await.unwrap();
        wait_for_connected(&mut events_a, session_id).await;
        wait_for_connected(&mut events_b, session_id).await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        directory_a.insert_peer_with_hints(
            peer_b,
            service_b.local_public_key(),
            Arc::from([SocketAddr::from(([203, 0, 113, 1], 9))]),
        );
        let started = Instant::now();
        handle_a.force_ice_restart(session_id).await.unwrap();
        wait_for_closed(&mut events_a, session_id).await;
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "restart removal exceeded its absolute deadline: {:?}",
            started.elapsed()
        );
        wait_for_absent(&handle_a, session_id).await;

        tokio::time::timeout(Duration::from_secs(5), async {
            let (a, b) = tokio::join!(service_a.shutdown(), service_b.shutdown());
            a.unwrap();
            b.unwrap();
        })
        .await
        .expect("timed-out restart services did not shut down");
    }

    async fn wait_for_incoming(
        events: &mut broadcast::Receiver<PeerSessionEvent>,
        peer_id: &PeerId,
    ) -> SessionId {
        match wait_for_event(events, |event| {
            matches!(event, PeerSessionEvent::IncomingRequest { peer_id: id, .. } if id == peer_id)
        })
        .await
        {
            PeerSessionEvent::IncomingRequest { session_id, .. } => session_id,
            _ => unreachable!(),
        }
    }

    async fn wait_for_connected(
        events: &mut broadcast::Receiver<PeerSessionEvent>,
        session_id: SessionId,
    ) {
        wait_for_event(events, |event| {
            matches!(event, PeerSessionEvent::Connected { session_id: id, .. } if *id == session_id)
        })
        .await;
    }

    async fn wait_for_closed(
        events: &mut broadcast::Receiver<PeerSessionEvent>,
        session_id: SessionId,
    ) {
        wait_for_event(events, |event| {
            matches!(event, PeerSessionEvent::Closed { session_id: id, .. } if *id == session_id)
        })
        .await;
    }

    async fn wait_for_event(
        events: &mut broadcast::Receiver<PeerSessionEvent>,
        predicate: impl Fn(&PeerSessionEvent) -> bool,
    ) -> PeerSessionEvent {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let event = events.recv().await.expect("session event channel closed");
                if predicate(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("timed out waiting for peer-session event")
    }

    async fn wait_for_absent(handle: &PeerSessionServiceHandle, session_id: SessionId) {
        let mut snapshots = handle.subscribe();
        tokio::time::timeout(Duration::from_secs(5), async {
            while snapshots.borrow().session(session_id).is_some() {
                snapshots.changed().await.expect("session snapshot channel closed");
            }
        })
        .await
        .expect("session was not removed");
    }

    async fn wait_for_transport_generation(
        handle: &PeerSessionServiceHandle,
        session_id: SessionId,
        previous: u64,
    ) {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let current = handle
                    .committed_transport_generation(session_id)
                    .await
                    .expect("session disappeared before committing its restart");
                if current > previous {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("session did not commit its next transport generation");
    }
}

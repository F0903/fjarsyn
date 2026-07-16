use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{Duration as ChronoDuration, Utc};
use futures_util::{SinkExt, StreamExt};
use socket2::SockRef;
use tokio::{
    net::{TcpListener, TcpSocket, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch},
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async_with_config, client_async_with_config,
    tungstenite::{
        Error as WebSocketError,
        handshake::server::{ErrorResponse, Request as WebSocketRequest, Response},
        http::StatusCode,
        protocol::{Message, WebSocketConfig},
    },
};

use super::{
    PeerId, PeerSessionError, SessionId,
    protocol::{
        EnvelopeVerification, NegotiationSignal, SessionReplayCache, SignedSessionEnvelope,
    },
    service::TrustedPeerResolver,
};
use crate::identity::{LocalPeerIdentity, TrustedPeerIdentity};

mod tls;

#[derive(Debug, Clone)]
pub(crate) struct NegotiationLimits {
    pub max_frame_bytes: usize,
    pub queue_capacity: usize,
    pub replay_capacity: usize,
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub authentication_global_burst: usize,
    pub authentication_global_refill_interval: Duration,
    pub authentication_per_ip_burst: usize,
    pub authentication_per_ip_refill_interval: Duration,
    pub max_authentication_tracked_ips: usize,
    pub handshake_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_message_age: ChronoDuration,
    pub max_clock_skew: ChronoDuration,
}

#[derive(Clone)]
struct ListenerConnectionContext {
    local_peer_id: PeerId,
    local_identity: LocalPeerIdentity,
    tls_acceptor: tls::IdentityTlsAcceptor,
    trusted_peers: Arc<dyn TrustedPeerResolver>,
    limits: NegotiationLimits,
    incoming_tx: mpsc::Sender<IncomingNegotiation>,
    request_replay: Arc<Mutex<SessionReplayCache>>,
}

struct SessionConnectionContext {
    session_id: SessionId,
    local_peer_id: PeerId,
    remote_peer_id: PeerId,
    local_identity: LocalPeerIdentity,
    trusted_peer: TrustedPeerIdentity,
    limits: NegotiationLimits,
}

#[derive(Default)]
struct ConnectionPermits {
    global: Option<OwnedSemaphorePermit>,
    ip: Option<IpConnectionPermit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthenticationRateLimitExceeded {
    Global,
    PerIp,
    TrackingCapacity,
}

#[derive(Debug)]
struct TokenBucket {
    capacity: usize,
    available: usize,
    refill_interval: Duration,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: usize, refill_interval: Duration, now: Instant) -> Self {
        debug_assert!(capacity > 0);
        debug_assert!(!refill_interval.is_zero());
        Self { capacity, available: capacity, refill_interval, last_refill: now }
    }

    fn refill(&mut self, now: Instant) {
        if self.available == self.capacity {
            self.last_refill = now;
            return;
        }

        let elapsed = now.saturating_duration_since(self.last_refill);
        let interval_nanos = self.refill_interval.as_nanos();
        let intervals = elapsed.as_nanos() / interval_nanos;
        if intervals == 0 {
            return;
        }

        let refill = usize::try_from(intervals).unwrap_or(usize::MAX);
        self.available = self.capacity.min(self.available.saturating_add(refill));
        if self.available == self.capacity {
            self.last_refill = now;
            return;
        }

        let remainder_nanos = elapsed.as_nanos() % interval_nanos;
        let remainder_seconds = u64::try_from(remainder_nanos / 1_000_000_000).unwrap_or(u64::MAX);
        let remainder_subseconds = (remainder_nanos % 1_000_000_000) as u32;
        let remainder = Duration::new(remainder_seconds, remainder_subseconds);
        self.last_refill = now.checked_sub(remainder).unwrap_or(now);
    }

    fn has_token(&self) -> bool {
        self.available > 0
    }

    fn take(&mut self) {
        debug_assert!(self.has_token());
        self.available -= 1;
    }

    fn is_full(&mut self, now: Instant) -> bool {
        self.refill(now);
        self.available == self.capacity
    }
}

#[derive(Debug)]
struct AuthenticationAttemptLimiter {
    global: TokenBucket,
    per_ip: HashMap<IpAddr, TokenBucket>,
    per_ip_burst: usize,
    per_ip_refill_interval: Duration,
    max_tracked_ips: usize,
    next_tracking_cleanup: Option<Instant>,
}

impl AuthenticationAttemptLimiter {
    fn new(limits: &NegotiationLimits, now: Instant) -> Self {
        Self {
            global: TokenBucket::new(
                limits.authentication_global_burst,
                limits.authentication_global_refill_interval,
                now,
            ),
            per_ip: HashMap::new(),
            per_ip_burst: limits.authentication_per_ip_burst,
            per_ip_refill_interval: limits.authentication_per_ip_refill_interval,
            max_tracked_ips: limits.max_authentication_tracked_ips,
            // `None` means the configured interval is beyond this platform's
            // representable Instant range. Such buckets cannot refill within
            // that range, so retaining them and failing closed is consistent.
            next_tracking_cleanup: now.checked_add(limits.authentication_per_ip_refill_interval),
        }
    }

    fn try_admit(
        &mut self,
        source_ip: IpAddr,
        now: Instant,
    ) -> Result<(), AuthenticationRateLimitExceeded> {
        let source_ip = source_ip.to_canonical();
        self.global.refill(now);
        if !self.global.has_token() {
            return Err(AuthenticationRateLimitExceeded::Global);
        }

        if let Some(bucket) = self.per_ip.get_mut(&source_ip) {
            bucket.refill(now);
            if !bucket.has_token() {
                return Err(AuthenticationRateLimitExceeded::PerIp);
            }
        } else {
            if self.per_ip.len() >= self.max_tracked_ips
                && self.next_tracking_cleanup.is_some_and(|deadline| now >= deadline)
            {
                self.per_ip.retain(|_, bucket| !bucket.is_full(now));
                self.next_tracking_cleanup = now.checked_add(self.per_ip_refill_interval);
            }
            if self.per_ip.len() >= self.max_tracked_ips {
                return Err(AuthenticationRateLimitExceeded::TrackingCapacity);
            }
            self.per_ip.insert(
                source_ip,
                TokenBucket::new(self.per_ip_burst, self.per_ip_refill_interval, now),
            );
        }

        self.global.take();
        self.per_ip.get_mut(&source_ip).expect("source bucket was admitted").take();
        Ok(())
    }
}

pub(crate) struct IncomingNegotiation {
    pub session_id: SessionId,
    pub peer_id: PeerId,
    pub authenticated_public_key: String,
    pub connection: NegotiationConnection,
}

impl std::fmt::Debug for IncomingNegotiation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IncomingNegotiation")
            .field("session_id", &self.session_id)
            .field("peer_id", &self.peer_id)
            .finish_non_exhaustive()
    }
}

pub(crate) struct NegotiationListener {
    port: u16,
    shutdown_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
    shutdown_timeout: Duration,
    joined: bool,
}

impl NegotiationListener {
    pub(crate) async fn bind(
        port: u16,
        local_peer_id: PeerId,
        local_identity: LocalPeerIdentity,
        trusted_peers: Arc<dyn TrustedPeerResolver>,
        limits: NegotiationLimits,
        incoming_tx: mpsc::Sender<IncomingNegotiation>,
    ) -> Result<Self, PeerSessionError> {
        let listener = bind_dual_stack_listener(port)?;
        let tls_acceptor = tls::IdentityTlsAcceptor::new(&local_identity)?;
        let port = listener
            .local_addr()
            .map_err(|error| PeerSessionError::Listener(error.to_string()))?
            .port();
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let semaphore = Arc::new(Semaphore::new(limits.max_connections.max(1)));
        let per_ip = Arc::new(Mutex::new(HashMap::<IpAddr, usize>::new()));
        let request_replay = Arc::new(Mutex::new(SessionReplayCache::new(limits.replay_capacity)));
        let shutdown_timeout = limits.handshake_timeout;

        let task = tokio::spawn(async move {
            let mut connection_tasks = JoinSet::new();
            let mut authentication_attempts =
                AuthenticationAttemptLimiter::new(&limits, Instant::now());
            loop {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = connection_tasks.join_next(), if !connection_tasks.is_empty() => {}
                    accepted = listener.accept() => {
                        let Ok((stream, address)) = accepted else {
                            if !*shutdown_rx.borrow() {
                                tracing::warn!("peer-session signaling listener accept failed");
                            }
                            continue;
                        };
                        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                            tracing::debug!(%address, "rejecting signaling connection at capacity");
                            continue;
                        };
                        let source_ip = address.ip().to_canonical();
                        let Some(ip_permit) = IpConnectionPermit::acquire(
                            per_ip.clone(),
                            source_ip,
                            limits.max_connections_per_ip,
                        ) else {
                            tracing::debug!(%address, "rejecting signaling connection at per-IP capacity");
                            continue;
                        };
                        if let Err(limit) =
                            authentication_attempts.try_admit(source_ip, Instant::now())
                        {
                            tracing::debug!(%source_ip, ?limit, "rejecting rate-limited signaling authentication attempt");
                            continue;
                        }
                        let context = ListenerConnectionContext {
                            local_peer_id: local_peer_id.clone(),
                            local_identity: local_identity.clone(),
                            tls_acceptor: tls_acceptor.clone(),
                            trusted_peers: trusted_peers.clone(),
                            limits: limits.clone(),
                            incoming_tx: incoming_tx.clone(),
                            request_replay: request_replay.clone(),
                        };
                        let permits = ConnectionPermits {
                            global: Some(permit),
                            ip: Some(ip_permit),
                        };
                        let task_shutdown = shutdown_rx.clone();
                        connection_tasks.spawn(async move {
                            if let Err(error) = accept_connection(
                                stream,
                                context,
                                permits,
                                task_shutdown,
                            ).await {
                                tracing::debug!(%address, %error, "rejected signaling connection");
                            }
                        });
                    }
                }
            }
            connection_tasks.abort_all();
            while connection_tasks.join_next().await.is_some() {}
        });

        Ok(Self { port, shutdown_tx, task, shutdown_timeout, joined: false })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) async fn shutdown(&mut self) {
        if self.joined {
            return;
        }
        let _ = self.shutdown_tx.send(true);
        if tokio::time::timeout(self.shutdown_timeout, &mut self.task).await.is_err() {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
        self.joined = true;
    }

    pub(crate) async fn abort_and_join(&mut self) {
        if self.joined {
            return;
        }
        let _ = self.shutdown_tx.send(true);
        self.task.abort();
        let _ = (&mut self.task).await;
        self.joined = true;
    }
}

impl Drop for NegotiationListener {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        if !self.joined {
            self.task.abort();
        }
    }
}

fn bind_dual_stack_listener(port: u16) -> Result<TcpListener, PeerSessionError> {
    let socket = TcpSocket::new_v6()
        .map_err(|error| PeerSessionError::Listener(format!("create IPv6 socket: {error}")))?;
    SockRef::from(&socket).set_only_v6(false).map_err(|error| {
        PeerSessionError::Listener(format!("enable dual-stack signaling: {error}"))
    })?;
    socket
        .bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port))
        .map_err(|error| PeerSessionError::Listener(format!("bind dual-stack socket: {error}")))?;
    socket.listen(1024).map_err(|error| {
        PeerSessionError::Listener(format!("listen on dual-stack socket: {error}"))
    })
}

pub(crate) struct NegotiationConnection {
    session_id: SessionId,
    local_peer_id: PeerId,
    remote_peer_id: PeerId,
    local_identity: LocalPeerIdentity,
    outbound_tx: mpsc::Sender<OutboundEnvelope>,
    inbound_rx: mpsc::Receiver<Result<NegotiationSignal, PeerSessionError>>,
    shutdown_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
    _permit: Option<OwnedSemaphorePermit>,
    _ip_permit: Option<IpConnectionPermit>,
    shutdown_timeout: Duration,
}

impl std::fmt::Debug for NegotiationConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NegotiationConnection")
            .field("session_id", &self.session_id)
            .field("local_peer_id", &self.local_peer_id)
            .field("remote_peer_id", &self.remote_peer_id)
            .finish_non_exhaustive()
    }
}

impl NegotiationConnection {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn connect_from_hints(
        endpoint_hints: &[SocketAddr],
        max_attempts: usize,
        attempt_timeout: Duration,
        session_id: SessionId,
        local_peer_id: PeerId,
        remote_peer_id: PeerId,
        local_identity: LocalPeerIdentity,
        trusted_peer: TrustedPeerIdentity,
        limits: NegotiationLimits,
    ) -> Result<Self, PeerSessionError> {
        if trusted_peer.peer_id != remote_peer_id {
            return Err(PeerSessionError::Protocol(
                "resolved trusted identity does not match the requested peer".into(),
            ));
        }
        trusted_peer.validate().map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
        let tls_connector = tls::PinnedTlsConnector::new(&trusted_peer)?;

        let endpoint_hints = plan_endpoint_hints(endpoint_hints, max_attempts);
        let mut attempted = 0;
        for endpoint in endpoint_hints {
            attempted += 1;
            let attempt = Self::connect_authenticated_endpoint(
                endpoint,
                tls_connector.clone(),
                session_id,
                local_peer_id.clone(),
                remote_peer_id.clone(),
                local_identity.clone(),
                trusted_peer.clone(),
                limits.clone(),
            );
            match tokio::time::timeout(attempt_timeout.max(Duration::from_millis(1)), attempt).await
            {
                Ok(Ok(connection)) => return Ok(connection),
                Ok(Err(error)) => {
                    tracing::debug!(
                        %remote_peer_id,
                        %endpoint,
                        %error,
                        "signaling endpoint hint failed authentication"
                    );
                }
                Err(_) => {
                    tracing::debug!(
                        %remote_peer_id,
                        %endpoint,
                        "signaling endpoint hint attempt timed out"
                    );
                }
            }
        }

        Err(PeerSessionError::EndpointAttemptsExhausted { peer_id: remote_peer_id, attempted })
    }

    #[allow(clippy::too_many_arguments)]
    async fn connect_authenticated_endpoint(
        endpoint: SocketAddr,
        tls_connector: tls::PinnedTlsConnector,
        session_id: SessionId,
        local_peer_id: PeerId,
        remote_peer_id: PeerId,
        local_identity: LocalPeerIdentity,
        trusted_peer: TrustedPeerIdentity,
        limits: NegotiationLimits,
    ) -> Result<Self, PeerSessionError> {
        let authenticating = async {
            let stream = TcpStream::connect(endpoint)
                .await
                .map_err(|error| PeerSessionError::Signaling(error.to_string()))?;
            stream
                .set_nodelay(true)
                .map_err(|error| PeerSessionError::Signaling(error.to_string()))?;
            let stream = tls_connector.connect(endpoint, stream).await?;
            let (mut socket, _) = client_async_with_config(
                secure_websocket_url(endpoint),
                stream,
                Some(websocket_config(&limits)),
            )
            .await
            .map_err(|error| PeerSessionError::Signaling(error.to_string()))?;
            let challenge = uuid::Uuid::new_v4();
            let hello = SignedSessionEnvelope::sign(
                &local_identity,
                session_id,
                local_peer_id.clone(),
                remote_peer_id.clone(),
                NegotiationSignal::EndpointHello { challenge },
                Utc::now(),
            )?;
            send_handshake_envelope(&mut socket, hello, &limits).await?;

            let proof = receive_handshake_envelope(&mut socket, &limits).await?;
            if !matches!(
                proof.payload(),
                NegotiationSignal::EndpointProof { challenge: received } if *received == challenge
            ) {
                return Err(PeerSessionError::Protocol(
                    "signaling endpoint returned an invalid identity proof".into(),
                ));
            }
            let mut replay = SessionReplayCache::new(limits.replay_capacity);
            proof.verify(
                EnvelopeVerification {
                    trusted_peer: &trusted_peer,
                    expected_local: &local_peer_id,
                    expected_remote: Some(&remote_peer_id),
                    expected_session: Some(session_id),
                    now: Utc::now(),
                    max_age: limits.max_message_age,
                    max_clock_skew: limits.max_clock_skew,
                },
                &mut replay,
            )?;
            Ok::<_, PeerSessionError>((socket, replay))
        };
        let (socket, replay) =
            tokio::time::timeout(limits.handshake_timeout, authenticating).await.map_err(
                |_| PeerSessionError::Signaling("signaling authentication timed out".into()),
            )??;

        Ok(spawn_socket(
            socket,
            SessionConnectionContext {
                session_id,
                local_peer_id,
                remote_peer_id,
                local_identity,
                trusted_peer,
                limits,
            },
            replay,
            ConnectionPermits::default(),
        ))
    }

    pub(crate) async fn send(&self, payload: NegotiationSignal) -> Result<(), PeerSessionError> {
        let envelope = SignedSessionEnvelope::sign(
            &self.local_identity,
            self.session_id,
            self.local_peer_id.clone(),
            self.remote_peer_id.clone(),
            payload,
            Utc::now(),
        )?;
        let (written_tx, written_rx) = oneshot::channel();
        self.outbound_tx
            .send(OutboundEnvelope { envelope, written: written_tx })
            .await
            .map_err(|_| PeerSessionError::Signaling("signaling connection closed".into()))?;
        written_rx
            .await
            .map_err(|_| PeerSessionError::Signaling("signaling writer stopped".into()))?
    }

    pub(crate) async fn recv(&mut self) -> Option<Result<NegotiationSignal, PeerSessionError>> {
        self.inbound_rx.recv().await
    }

    pub(crate) async fn shutdown(self) {
        let deadline = tokio::time::Instant::now() + self.shutdown_timeout;
        self.shutdown_until(deadline).await;
    }

    pub(crate) async fn shutdown_until(mut self, deadline: tokio::time::Instant) {
        let _ = self.shutdown_tx.send(true);
        if tokio::time::timeout_at(deadline, &mut self.task).await.is_err() {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}

impl Drop for NegotiationConnection {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.task.abort();
    }
}

async fn accept_connection(
    stream: TcpStream,
    context: ListenerConnectionContext,
    permits: ConnectionPermits,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), PeerSessionError> {
    let ListenerConnectionContext {
        local_peer_id,
        local_identity,
        tls_acceptor,
        trusted_peers,
        limits,
        incoming_tx,
        request_replay,
    } = context;
    let authentication_deadline = tokio::time::Instant::now() + limits.handshake_timeout;
    let authenticating = async {
        let stream = tls_acceptor.accept(stream).await?;
        let mut socket = accept_hdr_async_with_config(
            stream,
            validate_signaling_request,
            Some(websocket_config(&limits)),
        )
        .await
        .map_err(|error| PeerSessionError::Signaling(error.to_string()))?;
        let hello = receive_handshake_envelope(&mut socket, &limits).await?;
        let challenge = match hello.payload() {
            NegotiationSignal::EndpointHello { challenge } => *challenge,
            _ => {
                return Err(PeerSessionError::Protocol(
                    "first signaling message must authenticate the endpoint".into(),
                ));
            }
        };
        let peer_id = hello.from().clone();
        let session_id = hello.session_id();
        let trusted_peer = trusted_peers
            .trusted_peer(&peer_id)
            .await?
            .ok_or_else(|| PeerSessionError::PeerNotTrusted(peer_id.clone()))?;
        let verification = IncomingHandshakeVerification {
            trusted_peer: &trusted_peer,
            local_peer_id: &local_peer_id,
            remote_peer_id: &peer_id,
            session_id,
            limits: &limits,
            shared_replay: &request_replay,
        };
        let mut replay = SessionReplayCache::new(limits.replay_capacity);
        verify_incoming_handshake(&hello, &verification, &mut replay)?;

        let proof = SignedSessionEnvelope::sign(
            &local_identity,
            session_id,
            local_peer_id.clone(),
            peer_id.clone(),
            NegotiationSignal::EndpointProof { challenge },
            Utc::now(),
        )?;
        send_handshake_envelope(&mut socket, proof, &limits).await?;

        let request = receive_handshake_envelope(&mut socket, &limits).await?;
        if !matches!(request.payload(), NegotiationSignal::Request {}) {
            return Err(PeerSessionError::Protocol(
                "endpoint authentication must be followed by a connection request".into(),
            ));
        }
        verify_incoming_handshake(&request, &verification, &mut replay)?;
        Ok::<_, PeerSessionError>((socket, session_id, peer_id, trusted_peer, replay))
    };
    let (socket, session_id, peer_id, trusted_peer, replay) =
        tokio::time::timeout_at(authentication_deadline, authenticating).await.map_err(
            |_| PeerSessionError::Signaling("signaling authentication timed out".into()),
        )??;

    let authenticated_public_key = trusted_peer.public_key.clone();
    let connection = spawn_socket(
        socket,
        SessionConnectionContext {
            session_id,
            local_peer_id,
            remote_peer_id: peer_id.clone(),
            local_identity,
            trusted_peer,
            limits,
        },
        replay,
        permits,
    );
    let routing = incoming_tx.send(IncomingNegotiation {
        session_id,
        peer_id,
        authenticated_public_key,
        connection,
    });
    tokio::pin!(routing);
    tokio::select! {
        changed = shutdown_rx.changed() => {
            let _ = changed;
            Err(PeerSessionError::ServiceStopped)
        }
        result = tokio::time::timeout_at(authentication_deadline, &mut routing) => {
            result
                .map_err(|_| PeerSessionError::Signaling("incoming session routing timed out".into()))?
                .map_err(|_| PeerSessionError::ServiceStopped)
        }
    }
}

fn spawn_socket<S>(
    socket: WebSocketStream<S>,
    context: SessionConnectionContext,
    mut replay: SessionReplayCache,
    permits: ConnectionPermits,
) -> NegotiationConnection
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let SessionConnectionContext {
        session_id,
        local_peer_id,
        remote_peer_id,
        local_identity,
        trusted_peer,
        limits,
    } = context;
    let (mut writer, mut reader) = socket.split();
    let (outbound_tx, mut outbound_rx) =
        mpsc::channel::<OutboundEnvelope>(limits.queue_capacity.max(1));
    let (inbound_tx, inbound_rx) = mpsc::channel(limits.queue_capacity.max(1));
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let shutdown_timeout = limits.handshake_timeout;
    let expected_local = local_peer_id.clone();
    let expected_remote = remote_peer_id.clone();

    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        let _ = writer.send(Message::Close(None)).await;
                        break;
                    }
                }
                outbound = outbound_rx.recv() => {
                    let Some(outbound) = outbound else {
                        let _ = writer.send(Message::Close(None)).await;
                        break;
                    };
                    let serialized = match serde_json::to_string(&outbound.envelope) {
                        Ok(serialized) => serialized,
                        Err(error) => {
                            let error = PeerSessionError::Protocol(error.to_string());
                            let _ = outbound.written.send(Err(error.clone()));
                            let _ = inbound_tx.try_send(Err(error));
                            break;
                        }
                    };
                    match tokio::time::timeout(
                        limits.handshake_timeout,
                        writer.send(Message::Text(serialized.into())),
                    ).await {
                        Ok(Ok(())) => {
                            let _ = outbound.written.send(Ok(()));
                        }
                        Ok(Err(error)) => {
                            let error = PeerSessionError::Signaling(error.to_string());
                            let _ = outbound.written.send(Err(error.clone()));
                            let _ = inbound_tx.try_send(Err(error));
                            break;
                        }
                        Err(_) => {
                            let error = PeerSessionError::Signaling("signaling write timed out".into());
                            let _ = outbound.written.send(Err(error.clone()));
                            let _ = inbound_tx.try_send(Err(error));
                            break;
                        }
                    }
                }
                message = tokio::time::timeout(limits.idle_timeout, reader.next()) => {
                    let envelope = match message {
                        Err(_) => {
                            let _ = inbound_tx.try_send(Err(PeerSessionError::Signaling("signaling connection idle timeout".into())));
                            break;
                        }
                        Ok(None) => break,
                        Ok(Some(Err(error))) => {
                            if !matches!(error, WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) {
                                let _ = inbound_tx.try_send(Err(PeerSessionError::Signaling(error.to_string())));
                            }
                            break;
                        }
                        Ok(Some(Ok(message))) => match parse_envelope(message, limits.max_frame_bytes) {
                            Ok(envelope) => envelope,
                            Err(error) => {
                                let _ = inbound_tx.try_send(Err(error));
                                break;
                            }
                        }
                    };
                    if let Err(error) = envelope.verify(
                        EnvelopeVerification {
                            trusted_peer: &trusted_peer,
                            expected_local: &expected_local,
                            expected_remote: Some(&expected_remote),
                            expected_session: Some(session_id),
                            now: Utc::now(),
                            max_age: limits.max_message_age,
                            max_clock_skew: limits.max_clock_skew,
                        },
                        &mut replay,
                    ) {
                        let _ = inbound_tx.try_send(Err(error));
                        break;
                    }
                    match tokio::time::timeout(
                        limits.handshake_timeout,
                        inbound_tx.send(Ok(envelope.into_payload())),
                    ).await {
                        Ok(Ok(())) => {}
                        _ => break,
                    }
                }
            }
        }
    });

    NegotiationConnection {
        session_id,
        local_peer_id,
        remote_peer_id,
        local_identity,
        outbound_tx,
        inbound_rx,
        shutdown_tx,
        task,
        _permit: permits.global,
        _ip_permit: permits.ip,
        shutdown_timeout,
    }
}

struct OutboundEnvelope {
    envelope: SignedSessionEnvelope,
    written: oneshot::Sender<Result<(), PeerSessionError>>,
}

struct IpConnectionPermit {
    counts: Arc<Mutex<HashMap<IpAddr, usize>>>,
    ip: IpAddr,
}

impl IpConnectionPermit {
    fn acquire(
        counts: Arc<Mutex<HashMap<IpAddr, usize>>>,
        ip: IpAddr,
        limit: usize,
    ) -> Option<Self> {
        let mut locked = counts.lock().unwrap();
        let count = locked.entry(ip).or_default();
        if *count >= limit.max(1) {
            return None;
        }
        *count += 1;
        drop(locked);
        Some(Self { counts, ip })
    }
}

impl Drop for IpConnectionPermit {
    fn drop(&mut self) {
        let mut counts = self.counts.lock().unwrap();
        if let Some(count) = counts.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.ip);
            }
        }
    }
}

fn plan_endpoint_hints(endpoint_hints: &[SocketAddr], max_attempts: usize) -> Vec<SocketAddr> {
    let limit = max_attempts.max(1);
    let mut seen = HashSet::with_capacity(endpoint_hints.len().min(limit.saturating_add(1)));
    let mut planned = Vec::with_capacity(endpoint_hints.len().min(limit));
    for endpoint in endpoint_hints.iter().copied().filter_map(normalize_endpoint_hint) {
        if !seen.insert(endpoint) {
            continue;
        }
        if planned.len() < limit {
            planned.push(endpoint);
            if planned.len() < limit {
                continue;
            }
            if limit == 1
                || planned.iter().any(|candidate| candidate.is_ipv4() != planned[0].is_ipv4())
            {
                break;
            }
            // If the capped prefix contains only one address family, keep
            // scanning for one hint from the other family. Unauthenticated
            // claims in one family must not crowd the other family out of the
            // bounded attempt set entirely.
            continue;
        }

        if endpoint.is_ipv4() != planned[0].is_ipv4() {
            planned[limit - 1] = endpoint;
            break;
        }
    }
    planned
}

fn normalize_endpoint_hint(endpoint: SocketAddr) -> Option<SocketAddr> {
    if endpoint.port() == 0 {
        return None;
    }

    let endpoint = match endpoint {
        SocketAddr::V6(address) => match address.ip().to_ipv4_mapped() {
            Some(address) => SocketAddr::new(IpAddr::V4(address), endpoint.port()),
            None => SocketAddr::V6(address),
        },
        endpoint => endpoint,
    };
    match endpoint.ip() {
        IpAddr::V4(address)
            if address.is_unspecified() || address.is_multicast() || address.is_broadcast() =>
        {
            None
        }
        IpAddr::V6(address) if address.is_unspecified() || address.is_multicast() => None,
        _ => Some(endpoint),
    }
}

fn secure_websocket_url(endpoint: SocketAddr) -> String {
    match endpoint {
        SocketAddr::V6(address) if address.scope_id() != 0 => {
            format!("wss://[{}%25{}]:{}/session", address.ip(), address.scope_id(), address.port())
        }
        endpoint => format!("wss://{endpoint}/session"),
    }
}

struct IncomingHandshakeVerification<'a> {
    trusted_peer: &'a TrustedPeerIdentity,
    local_peer_id: &'a PeerId,
    remote_peer_id: &'a PeerId,
    session_id: SessionId,
    limits: &'a NegotiationLimits,
    shared_replay: &'a Arc<Mutex<SessionReplayCache>>,
}

fn verify_incoming_handshake(
    envelope: &SignedSessionEnvelope,
    context: &IncomingHandshakeVerification<'_>,
    connection_replay: &mut SessionReplayCache,
) -> Result<(), PeerSessionError> {
    let verification = || EnvelopeVerification {
        trusted_peer: context.trusted_peer,
        expected_local: context.local_peer_id,
        expected_remote: Some(context.remote_peer_id),
        expected_session: Some(context.session_id),
        now: Utc::now(),
        max_age: context.limits.max_message_age,
        max_clock_skew: context.limits.max_clock_skew,
    };
    envelope.verify(verification(), &mut context.shared_replay.lock().unwrap())?;
    envelope.verify(verification(), connection_replay)
}

async fn send_handshake_envelope<S>(
    socket: &mut WebSocketStream<S>,
    envelope: SignedSessionEnvelope,
    limits: &NegotiationLimits,
) -> Result<(), PeerSessionError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let serialized = serde_json::to_string(&envelope)
        .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
    if serialized.len() > limits.max_frame_bytes {
        return Err(PeerSessionError::Protocol(
            "signaling handshake frame exceeds size limit".into(),
        ));
    }
    tokio::time::timeout(limits.handshake_timeout, socket.send(Message::Text(serialized.into())))
        .await
        .map_err(|_| PeerSessionError::Signaling("signaling handshake write timed out".into()))?
        .map_err(|error| PeerSessionError::Signaling(error.to_string()))
}

async fn receive_handshake_envelope<S>(
    socket: &mut WebSocketStream<S>,
    limits: &NegotiationLimits,
) -> Result<SignedSessionEnvelope, PeerSessionError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(limits.handshake_timeout, socket.next())
        .await
        .map_err(|_| PeerSessionError::Signaling("signaling handshake read timed out".into()))?
        .ok_or_else(|| PeerSessionError::Signaling("signaling connection closed".into()))?
        .map_err(|error| PeerSessionError::Signaling(error.to_string()))?;
    parse_envelope(message, limits.max_frame_bytes)
}

fn websocket_config(limits: &NegotiationLimits) -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(limits.max_frame_bytes))
        .max_frame_size(Some(limits.max_frame_bytes))
}

// Tungstenite's callback contract requires its concrete HTTP error response;
// the large error type cannot be boxed without violating that trait signature.
#[allow(clippy::result_large_err)]
fn validate_signaling_request(
    request: &WebSocketRequest,
    response: Response,
) -> Result<Response, ErrorResponse> {
    if request.uri().path() == "/session" && request.uri().query().is_none() {
        return Ok(response);
    }

    let mut rejection = ErrorResponse::new(Some("unknown signaling endpoint".into()));
    *rejection.status_mut() = StatusCode::NOT_FOUND;
    Err(rejection)
}

fn parse_envelope(
    message: Message,
    max_frame_bytes: usize,
) -> Result<SignedSessionEnvelope, PeerSessionError> {
    let Message::Text(text) = message else {
        return Err(PeerSessionError::Protocol("signaling frames must be UTF-8 text".into()));
    };
    if text.len() > max_frame_bytes {
        return Err(PeerSessionError::Protocol("signaling frame exceeds size limit".into()));
    }
    serde_json::from_str(&text).map_err(|error| PeerSessionError::Protocol(error.to_string()))
}

// Keep the concrete outgoing socket type checked by the compiler. This catches
// accidental assumptions in `spawn_socket` when tokio-tungstenite changes.
#[allow(dead_code)]
fn _outgoing_socket_type(_: WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>) {}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, SocketAddrV6},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    use super::*;

    #[derive(Debug)]
    struct FixedTrustedPeer(TrustedPeerIdentity);

    #[async_trait]
    impl TrustedPeerResolver for FixedTrustedPeer {
        async fn trusted_peer(
            &self,
            peer_id: &PeerId,
        ) -> Result<Option<TrustedPeerIdentity>, PeerSessionError> {
            Ok((&self.0.peer_id == peer_id).then(|| self.0.clone()))
        }
    }

    #[derive(Debug)]
    struct CountingTrustedPeer {
        trusted: TrustedPeerIdentity,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl TrustedPeerResolver for CountingTrustedPeer {
        async fn trusted_peer(
            &self,
            peer_id: &PeerId,
        ) -> Result<Option<TrustedPeerIdentity>, PeerSessionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((&self.trusted.peer_id == peer_id).then(|| self.trusted.clone()))
        }
    }

    fn test_limits() -> NegotiationLimits {
        NegotiationLimits {
            max_frame_bytes: 16 * 1024,
            queue_capacity: 8,
            replay_capacity: 8,
            max_connections: 4,
            max_connections_per_ip: 4,
            authentication_global_burst: 32,
            authentication_global_refill_interval: Duration::from_millis(100),
            authentication_per_ip_burst: 8,
            authentication_per_ip_refill_interval: Duration::from_millis(500),
            max_authentication_tracked_ips: 32,
            handshake_timeout: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(5),
            max_message_age: ChronoDuration::minutes(5),
            max_clock_skew: ChronoDuration::seconds(30),
        }
    }

    async fn connect_raw_socket(
        endpoint: SocketAddr,
        trusted_listener: &TrustedPeerIdentity,
        limits: &NegotiationLimits,
    ) -> Result<WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>, PeerSessionError> {
        let connecting = async {
            let stream = TcpStream::connect(endpoint)
                .await
                .map_err(|error| PeerSessionError::Signaling(error.to_string()))?;
            let stream =
                tls::PinnedTlsConnector::new(trusted_listener)?.connect(endpoint, stream).await?;
            let (socket, _) = client_async_with_config(
                secure_websocket_url(endpoint),
                stream,
                Some(websocket_config(limits)),
            )
            .await
            .map_err(|error| PeerSessionError::Signaling(error.to_string()))?;
            Ok::<_, PeerSessionError>(socket)
        };
        tokio::time::timeout(limits.handshake_timeout, connecting)
            .await
            .map_err(|_| PeerSessionError::Signaling("test WSS handshake timed out".into()))?
    }

    #[test]
    fn authentication_limiter_enforces_global_and_per_ip_bursts_atomically() {
        let now = Instant::now();
        let first_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let second_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 11));

        let mut limits = test_limits();
        limits.authentication_global_burst = 3;
        limits.authentication_per_ip_burst = 1;
        let mut limiter = AuthenticationAttemptLimiter::new(&limits, now);

        assert_eq!(limiter.try_admit(first_ip, now), Ok(()));
        let global_after_first = limiter.global.available;
        assert_eq!(limiter.try_admit(first_ip, now), Err(AuthenticationRateLimitExceeded::PerIp));
        assert_eq!(limiter.global.available, global_after_first);

        assert_eq!(limiter.try_admit(second_ip, now), Ok(()));
        let second_available = limiter.per_ip[&second_ip].available;
        assert_eq!(limiter.try_admit(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 12)), now), Ok(()));
        assert_eq!(limiter.try_admit(second_ip, now), Err(AuthenticationRateLimitExceeded::Global));
        assert_eq!(limiter.per_ip[&second_ip].available, second_available);
    }

    #[test]
    fn authentication_limiter_refills_at_exact_intervals_without_overfilling() {
        let now = Instant::now();
        let source_ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mut limits = test_limits();
        limits.authentication_global_burst = 2;
        limits.authentication_global_refill_interval = Duration::from_millis(100);
        limits.authentication_per_ip_burst = 2;
        limits.authentication_per_ip_refill_interval = Duration::from_millis(100);
        let mut limiter = AuthenticationAttemptLimiter::new(&limits, now);

        assert_eq!(limiter.try_admit(source_ip, now), Ok(()));
        assert_eq!(limiter.try_admit(source_ip, now), Ok(()));
        assert_eq!(
            limiter.try_admit(source_ip, now + Duration::from_millis(99)),
            Err(AuthenticationRateLimitExceeded::Global)
        );
        assert_eq!(limiter.try_admit(source_ip, now + Duration::from_millis(100)), Ok(()));
        assert_eq!(
            limiter.try_admit(source_ip, now + Duration::from_millis(100)),
            Err(AuthenticationRateLimitExceeded::Global)
        );

        let much_later = now + Duration::from_secs(10);
        assert_eq!(limiter.try_admit(source_ip, much_later), Ok(()));
        assert_eq!(limiter.try_admit(source_ip, much_later), Ok(()));
        assert_eq!(
            limiter.try_admit(source_ip, much_later),
            Err(AuthenticationRateLimitExceeded::Global)
        );
    }

    #[test]
    fn authentication_limiter_canonicalizes_mapped_ipv4_sources() {
        let now = Instant::now();
        let mut limits = test_limits();
        limits.authentication_per_ip_burst = 1;
        let mut limiter = AuthenticationAttemptLimiter::new(&limits, now);
        let ipv4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mapped = IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped());

        assert_eq!(limiter.try_admit(ipv4, now), Ok(()));
        assert_eq!(limiter.try_admit(mapped, now), Err(AuthenticationRateLimitExceeded::PerIp));
        assert_eq!(limiter.per_ip.len(), 1);
        assert!(limiter.per_ip.contains_key(&ipv4));
    }

    #[test]
    fn authentication_limiter_bounds_source_tracking_and_reuses_refilled_slots() {
        let now = Instant::now();
        let mut limits = test_limits();
        limits.authentication_global_burst = 8;
        limits.authentication_global_refill_interval = Duration::from_secs(1);
        limits.authentication_per_ip_burst = 1;
        limits.authentication_per_ip_refill_interval = Duration::from_secs(1);
        limits.max_authentication_tracked_ips = 2;
        let mut limiter = AuthenticationAttemptLimiter::new(&limits, now);
        let first = IpAddr::V6("fd00::1".parse().unwrap());
        let second = IpAddr::V6("fd00::2".parse().unwrap());
        let third = IpAddr::V6("fd00::3".parse().unwrap());

        assert_eq!(limiter.try_admit(first, now), Ok(()));
        assert_eq!(limiter.try_admit(second, now), Ok(()));
        assert_eq!(
            limiter.try_admit(third, now),
            Err(AuthenticationRateLimitExceeded::TrackingCapacity)
        );
        assert_eq!(limiter.per_ip.len(), 2);
        assert_eq!(limiter.try_admit(third, now + Duration::from_secs(1)), Ok(()));
        assert_eq!(limiter.per_ip.len(), 1);
        assert!(limiter.per_ip.contains_key(&third));
    }

    #[tokio::test]
    async fn listener_rate_limits_malformed_and_untrusted_attempts_before_trust_work() {
        let local_peer = PeerId::new("local").unwrap();
        let trusted_peer = PeerId::new("trusted").unwrap();
        let unknown_peer = PeerId::new("unknown").unwrap();
        let local_identity = LocalPeerIdentity::generate();
        let trusted_listener =
            TrustedPeerIdentity::new(local_peer.clone(), local_identity.public_key_base64());
        let trusted_identity = LocalPeerIdentity::generate();
        let unknown_identity = LocalPeerIdentity::generate();
        let trusted = Arc::new(CountingTrustedPeer {
            trusted: TrustedPeerIdentity::new(
                trusted_peer.clone(),
                trusted_identity.public_key_base64(),
            ),
            calls: AtomicUsize::new(0),
        });
        let mut limits = test_limits();
        limits.authentication_global_burst = 2;
        limits.authentication_global_refill_interval = Duration::from_secs(60);
        limits.authentication_per_ip_burst = 2;
        limits.authentication_per_ip_refill_interval = Duration::from_secs(60);
        let (incoming_tx, mut incoming_rx) = mpsc::channel(1);
        let mut listener = NegotiationListener::bind(
            0,
            local_peer.clone(),
            local_identity,
            trusted.clone(),
            limits.clone(),
            incoming_tx,
        )
        .await
        .unwrap();
        let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, listener.port()));

        let mut malformed = connect_raw_socket(endpoint, &trusted_listener, &limits).await.unwrap();
        malformed.send(Message::Text("not-json".into())).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), malformed.next()).await;
        assert_eq!(trusted.calls.load(Ordering::SeqCst), 0);

        let mut unknown = connect_raw_socket(endpoint, &trusted_listener, &limits).await.unwrap();
        let unknown_hello = SignedSessionEnvelope::sign(
            &unknown_identity,
            SessionId::new(),
            unknown_peer,
            local_peer,
            NegotiationSignal::EndpointHello { challenge: uuid::Uuid::new_v4() },
            Utc::now(),
        )
        .unwrap();
        send_handshake_envelope(&mut unknown, unknown_hello, &limits).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), unknown.next()).await;
        assert_eq!(trusted.calls.load(Ordering::SeqCst), 1);

        let third = tokio::time::timeout(
            Duration::from_secs(1),
            connect_raw_socket(endpoint, &trusted_listener, &limits),
        )
        .await
        .expect("rate-limited WSS handshake must terminate promptly");
        assert!(third.is_err());
        assert_eq!(trusted.calls.load(Ordering::SeqCst), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), incoming_rx.recv()).await.is_err()
        );

        listener.shutdown().await;
    }

    #[tokio::test]
    async fn listener_rejects_plaintext_websocket_before_trust_resolution() {
        let local_peer = PeerId::new("local").unwrap();
        let remote_identity = LocalPeerIdentity::generate();
        let trusted = Arc::new(CountingTrustedPeer {
            trusted: TrustedPeerIdentity::new(
                PeerId::new("remote").unwrap(),
                remote_identity.public_key_base64(),
            ),
            calls: AtomicUsize::new(0),
        });
        let (incoming_tx, mut incoming_rx) = mpsc::channel(1);
        let mut listener = NegotiationListener::bind(
            0,
            local_peer,
            LocalPeerIdentity::generate(),
            trusted.clone(),
            test_limits(),
            incoming_tx,
        )
        .await
        .unwrap();

        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, listener.port())).await.unwrap();
        stream.write_all(b"GET /session HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        stream.shutdown().await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut response))
            .await
            .expect("plaintext signaling connection must close promptly")
            .unwrap();

        for plaintext_protocol_marker in [b"HTTP/1.1".as_slice(), b"Sec-WebSocket-Accept"] {
            assert!(
                !response
                    .windows(plaintext_protocol_marker.len())
                    .any(|window| window == plaintext_protocol_marker)
            );
        }
        assert_eq!(trusted.calls.load(Ordering::SeqCst), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), incoming_rx.recv()).await.is_err()
        );
        listener.shutdown().await;
    }

    #[tokio::test]
    async fn signed_request_replay_is_rejected_across_websocket_connections() {
        let local_peer = PeerId::new("local").unwrap();
        let remote_peer = PeerId::new("remote").unwrap();
        let local_identity = LocalPeerIdentity::generate();
        let trusted_listener =
            TrustedPeerIdentity::new(local_peer.clone(), local_identity.public_key_base64());
        let remote_identity = LocalPeerIdentity::generate();
        let trusted = Arc::new(FixedTrustedPeer(TrustedPeerIdentity::new(
            remote_peer.clone(),
            remote_identity.public_key_base64(),
        )));
        let (incoming_tx, mut incoming_rx) = mpsc::channel(2);
        let mut listener = NegotiationListener::bind(
            0,
            local_peer.clone(),
            local_identity,
            trusted,
            test_limits(),
            incoming_tx,
        )
        .await
        .unwrap();
        let session_id = SessionId::new();
        let envelope = SignedSessionEnvelope::sign(
            &remote_identity,
            session_id,
            remote_peer.clone(),
            local_peer.clone(),
            NegotiationSignal::Request {},
            Utc::now(),
        )
        .unwrap();
        let encoded = serde_json::to_string(&envelope).unwrap();

        let endpoint = SocketAddr::from((Ipv4Addr::LOCALHOST, listener.port()));
        let mut first_socket =
            connect_raw_socket(endpoint, &trusted_listener, &test_limits()).await.unwrap();
        authenticate_raw_socket(
            &mut first_socket,
            &remote_identity,
            &remote_peer,
            &local_peer,
            session_id,
        )
        .await;
        first_socket.send(Message::Text(encoded.clone().into())).await.unwrap();
        let first = tokio::time::timeout(Duration::from_secs(1), incoming_rx.recv())
            .await
            .unwrap()
            .unwrap();

        let mut replay_socket =
            connect_raw_socket(endpoint, &trusted_listener, &test_limits()).await.unwrap();
        authenticate_raw_socket(
            &mut replay_socket,
            &remote_identity,
            &remote_peer,
            &local_peer,
            session_id,
        )
        .await;
        replay_socket.send(Message::Text(encoded.into())).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(150), incoming_rx.recv()).await.is_err()
        );

        first.connection.shutdown().await;
        let _ = first_socket.close(None).await;
        let _ = replay_socket.close(None).await;
        listener.shutdown().await;
    }

    async fn authenticate_raw_socket<S>(
        socket: &mut WebSocketStream<S>,
        identity: &LocalPeerIdentity,
        local_peer: &PeerId,
        remote_peer: &PeerId,
        session_id: SessionId,
    ) where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let limits = test_limits();
        let challenge = uuid::Uuid::new_v4();
        let hello = SignedSessionEnvelope::sign(
            identity,
            session_id,
            local_peer.clone(),
            remote_peer.clone(),
            NegotiationSignal::EndpointHello { challenge },
            Utc::now(),
        )
        .unwrap();
        send_handshake_envelope(socket, hello, &limits).await.unwrap();
        let proof = receive_handshake_envelope(socket, &limits).await.unwrap();
        assert!(matches!(
            proof.payload(),
            NegotiationSignal::EndpointProof { challenge: received } if *received == challenge
        ));
    }

    #[tokio::test]
    async fn one_listener_authenticates_ipv4_and_ipv6_connections() {
        let listener_peer = PeerId::new("listener").unwrap();
        let client_peer = PeerId::new("client").unwrap();
        let listener_identity = LocalPeerIdentity::generate();
        let client_identity = LocalPeerIdentity::generate();
        let trusted_client = Arc::new(FixedTrustedPeer(TrustedPeerIdentity::new(
            client_peer.clone(),
            client_identity.public_key_base64(),
        )));
        let trusted_listener =
            TrustedPeerIdentity::new(listener_peer.clone(), listener_identity.public_key_base64());
        let (incoming_tx, mut incoming_rx) = mpsc::channel(2);
        let mut listener = NegotiationListener::bind(
            0,
            listener_peer.clone(),
            listener_identity,
            trusted_client,
            test_limits(),
            incoming_tx,
        )
        .await
        .unwrap();

        let endpoints = [
            SocketAddr::from((Ipv4Addr::LOCALHOST, listener.port())),
            SocketAddr::from((Ipv6Addr::LOCALHOST, listener.port())),
        ];
        for endpoint in endpoints {
            let session_id = SessionId::new();
            let connection = NegotiationConnection::connect_from_hints(
                &[endpoint],
                1,
                Duration::from_secs(1),
                session_id,
                client_peer.clone(),
                listener_peer.clone(),
                client_identity.clone(),
                trusted_listener.clone(),
                test_limits(),
            )
            .await
            .unwrap();
            connection.send(NegotiationSignal::Request {}).await.unwrap();
            let incoming = tokio::time::timeout(Duration::from_secs(1), incoming_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(incoming.session_id, session_id);
            incoming.connection.shutdown().await;
            connection.shutdown().await;
        }

        listener.shutdown().await;
    }

    #[test]
    fn endpoint_plan_is_stable_deduplicated_capped_and_rejects_unusable_hints() {
        let first = SocketAddr::from((Ipv4Addr::LOCALHOST, 9000));
        let mapped = SocketAddr::V6(std::net::SocketAddrV6::new(
            Ipv4Addr::LOCALHOST.to_ipv6_mapped(),
            first.port(),
            0,
            0,
        ));
        let second = SocketAddr::from(([192, 168, 1, 10], 9000));
        let third = SocketAddr::from((Ipv6Addr::LOCALHOST, 9000));
        let hints = [SocketAddr::from(([0, 0, 0, 0], 9000)), first, mapped, first, second, third];

        assert_eq!(plan_endpoint_hints(&hints, 2), vec![first, third]);
        assert_eq!(plan_endpoint_hints(&[first, second], 2), vec![first, second]);
        assert_eq!(plan_endpoint_hints(&[first], usize::MAX), vec![first]);
    }

    #[test]
    fn scoped_ipv6_websocket_url_is_valid_and_retains_the_zone_index() {
        let endpoint =
            SocketAddr::V6(SocketAddrV6::new("fe80::1234".parse().unwrap(), 9000, 0, 17));
        let url = secure_websocket_url(endpoint);

        assert_eq!(url, "wss://[fe80::1234%2517]:9000/session");
        assert!(url.into_client_request().is_ok());
    }

    #[test]
    fn signaling_websocket_request_target_is_exact() {
        let request = WebSocketRequest::builder().uri("/session").body(()).unwrap();
        assert!(validate_signaling_request(&request, Response::new(())).is_ok());

        for invalid_target in ["/", "/session/", "/session?unexpected=true"] {
            let request = WebSocketRequest::builder().uri(invalid_target).body(()).unwrap();
            let rejection = validate_signaling_request(&request, Response::new(())).unwrap_err();
            assert_eq!(rejection.status(), StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn listener_shutdown_cancels_incomplete_handshakes() {
        let identity = LocalPeerIdentity::generate();
        let trusted = Arc::new(FixedTrustedPeer(TrustedPeerIdentity::new(
            PeerId::new("remote").unwrap(),
            LocalPeerIdentity::generate().public_key_base64(),
        )));
        let (incoming_tx, _incoming_rx) = mpsc::channel(1);
        let mut listener = NegotiationListener::bind(
            0,
            PeerId::new("local").unwrap(),
            identity,
            trusted,
            test_limits(),
            incoming_tx,
        )
        .await
        .unwrap();
        let _stalled = TcpStream::connect((Ipv4Addr::LOCALHOST, listener.port())).await.unwrap();

        tokio::time::timeout(Duration::from_secs(1), listener.shutdown())
            .await
            .expect("listener shutdown must join handshake tasks");
    }
}

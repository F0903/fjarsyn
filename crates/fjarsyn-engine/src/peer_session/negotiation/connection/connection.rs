use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::Utc;
use tokio::{net::TcpStream, sync::OwnedSemaphorePermit};
use tokio_tungstenite::{WebSocketStream, client_async_with_config};

use super::{
    endpoint_plan::{plan_endpoint_hints, secure_websocket_url},
    handshake::{receive_handshake_envelope, send_handshake_envelope, websocket_config},
    socket_runtime::SocketRuntime,
};
use crate::{
    identity::{LocalPeerIdentity, PeerId, TrustedPeerIdentity},
    peer_session::{
        Error, SessionId,
        negotiation::{Limits, tls},
        protocol::{
            EnvelopeVerification, NegotiationSignal, SessionReplayCache, SignedSessionEnvelope,
        },
    },
};

#[derive(Default)]
pub(in crate::peer_session::negotiation) struct ConnectionPermits {
    pub(in crate::peer_session::negotiation) global: Option<OwnedSemaphorePermit>,
    pub(in crate::peer_session::negotiation) ip: Option<IpConnectionPermit>,
}

pub(in crate::peer_session::negotiation) struct IpConnectionPermit {
    counts: Arc<Mutex<HashMap<IpAddr, usize>>>,
    ip: IpAddr,
}

impl IpConnectionPermit {
    pub(in crate::peer_session::negotiation) fn acquire(
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

#[derive(Clone)]
pub(in crate::peer_session::negotiation) struct SessionConnectionContext {
    pub(in crate::peer_session::negotiation) session_id: SessionId,
    pub(in crate::peer_session::negotiation) local_peer_id: PeerId,
    pub(in crate::peer_session::negotiation) remote_peer_id: PeerId,
    pub(in crate::peer_session::negotiation) local_identity: LocalPeerIdentity,
    pub(in crate::peer_session::negotiation) trusted_peer: TrustedPeerIdentity,
    pub(in crate::peer_session::negotiation) limits: Limits,
}

pub(in crate::peer_session) struct Connection {
    session_id: SessionId,
    local_peer_id: PeerId,
    remote_peer_id: PeerId,
    authenticated_remote_public_key: String,
    local_identity: LocalPeerIdentity,
    runtime: SocketRuntime,
    _permit: Option<OwnedSemaphorePermit>,
    _ip_permit: Option<IpConnectionPermit>,
    shutdown_timeout: Duration,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Connection")
            .field("session_id", &self.session_id)
            .field("local_peer_id", &self.local_peer_id)
            .field("remote_peer_id", &self.remote_peer_id)
            .finish_non_exhaustive()
    }
}

impl Connection {
    pub(in crate::peer_session) fn authenticated_remote_public_key(&self) -> &str {
        &self.authenticated_remote_public_key
    }

    pub(in crate::peer_session::negotiation) async fn connect_from_hints(
        endpoint_hints: &[SocketAddr],
        context: SessionConnectionContext,
    ) -> Result<Self, Error> {
        if context.trusted_peer.peer_id != context.remote_peer_id {
            return Err(Error::Protocol(
                "resolved trusted identity does not match the requested peer".into(),
            ));
        }
        context.trusted_peer.validate().map_err(|error| Error::Protocol(error.to_string()))?;
        let tls_connector = tls::Connector::new(&context.trusted_peer)?;

        let endpoint_hints =
            plan_endpoint_hints(endpoint_hints, context.limits.max_endpoint_attempts);
        let mut attempted = 0;
        for endpoint in endpoint_hints {
            attempted += 1;
            let attempt = Self::connect_authenticated_endpoint(
                endpoint,
                tls_connector.clone(),
                context.clone(),
            );
            match tokio::time::timeout(
                context.limits.endpoint_attempt_timeout.max(Duration::from_millis(1)),
                attempt,
            )
            .await
            {
                Ok(Ok(connection)) => return Ok(connection),
                Ok(Err(error)) => {
                    tracing::debug!(
                        remote_peer_id = %context.remote_peer_id,
                        %endpoint,
                        %error,
                        "signaling endpoint hint failed authentication"
                    );
                }
                Err(_) => {
                    tracing::debug!(
                        remote_peer_id = %context.remote_peer_id,
                        %endpoint,
                        "signaling endpoint hint attempt timed out"
                    );
                }
            }
        }

        Err(Error::EndpointAttemptsExhausted { peer_id: context.remote_peer_id, attempted })
    }

    async fn connect_authenticated_endpoint(
        endpoint: SocketAddr,
        tls_connector: tls::Connector,
        context: SessionConnectionContext,
    ) -> Result<Self, Error> {
        let authenticating = async {
            let stream = TcpStream::connect(endpoint)
                .await
                .map_err(|error| Error::Signaling(error.to_string()))?;
            stream.set_nodelay(true).map_err(|error| Error::Signaling(error.to_string()))?;
            let stream = tls_connector.connect(endpoint, stream).await?;
            let (mut socket, _) = client_async_with_config(
                secure_websocket_url(endpoint),
                stream,
                Some(websocket_config(&context.limits)),
            )
            .await
            .map_err(|error| Error::Signaling(error.to_string()))?;
            let challenge = uuid::Uuid::new_v4();
            let hello = SignedSessionEnvelope::sign(
                &context.local_identity,
                context.session_id,
                context.local_peer_id.clone(),
                context.remote_peer_id.clone(),
                NegotiationSignal::EndpointHello { challenge },
                Utc::now(),
            )?;
            send_handshake_envelope(&mut socket, hello, &context.limits).await?;

            let proof = receive_handshake_envelope(&mut socket, &context.limits).await?;
            if !matches!(
                proof.payload(),
                NegotiationSignal::EndpointProof { challenge: received } if *received == challenge
            ) {
                return Err(Error::Protocol(
                    "signaling endpoint returned an invalid identity proof".into(),
                ));
            }
            let mut replay = SessionReplayCache::new(context.limits.replay_capacity);
            proof.verify(
                EnvelopeVerification {
                    trusted_peer: &context.trusted_peer,
                    expected_local: &context.local_peer_id,
                    expected_remote: Some(&context.remote_peer_id),
                    expected_session: Some(context.session_id),
                    now: Utc::now(),
                    max_age: context.limits.max_message_age,
                    max_clock_skew: context.limits.max_clock_skew,
                },
                &mut replay,
            )?;
            Ok::<_, Error>((socket, replay))
        };
        let (socket, replay) =
            tokio::time::timeout(context.limits.handshake_timeout, authenticating)
                .await
                .map_err(|_| Error::Signaling("signaling authentication timed out".into()))??;

        Ok(Self::from_socket(socket, context, replay, ConnectionPermits::default()))
    }

    pub(in crate::peer_session::negotiation) fn from_socket<S>(
        socket: WebSocketStream<S>,
        context: SessionConnectionContext,
        replay: SessionReplayCache,
        permits: ConnectionPermits,
    ) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let session_id = context.session_id;
        let local_peer_id = context.local_peer_id.clone();
        let remote_peer_id = context.remote_peer_id.clone();
        let authenticated_remote_public_key = context.trusted_peer.public_key.clone();
        let local_identity = context.local_identity.clone();
        let shutdown_timeout = context.limits.handshake_timeout;
        let runtime = SocketRuntime::spawn(socket, context, replay);

        Self {
            session_id,
            local_peer_id,
            remote_peer_id,
            authenticated_remote_public_key,
            local_identity,
            runtime,
            _permit: permits.global,
            _ip_permit: permits.ip,
            shutdown_timeout,
        }
    }

    pub(in crate::peer_session) async fn send(
        &self,
        payload: NegotiationSignal,
    ) -> Result<(), Error> {
        let envelope = SignedSessionEnvelope::sign(
            &self.local_identity,
            self.session_id,
            self.local_peer_id.clone(),
            self.remote_peer_id.clone(),
            payload,
            Utc::now(),
        )?;
        self.runtime.send(envelope).await
    }

    pub(in crate::peer_session) async fn recv(
        &mut self,
    ) -> Option<Result<NegotiationSignal, Error>> {
        self.runtime.recv().await
    }

    pub(in crate::peer_session) async fn shutdown(self) {
        let deadline = tokio::time::Instant::now() + self.shutdown_timeout;
        self.shutdown_until(deadline).await;
    }

    pub(in crate::peer_session) async fn shutdown_until(mut self, deadline: tokio::time::Instant) {
        self.runtime.shutdown_until(deadline).await;
    }
}

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::Utc;
use futures::FutureExt;
use socket2::SockRef;
use tokio::{
    net::{TcpListener, TcpSocket, TcpStream},
    sync::{Semaphore, mpsc, watch},
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        handshake::server::{ErrorResponse, Request as WebSocketRequest, Response},
        http::StatusCode,
    },
};

use super::{
    Incoming, Intent, Limits,
    admission::AuthenticationAttemptLimiter,
    connection::{
        Connection, ConnectionPermits, IpConnectionPermit, SessionConnectionContext,
        receive_handshake_envelope, send_handshake_envelope, websocket_config,
    },
    tls,
};
use crate::{
    identity::{LocalPeerIdentity, PeerId, TrustedPeerIdentity},
    peer_session::{
        Error, SessionId, TrustedPeerResolver,
        protocol::{
            EnvelopeVerification, NegotiationSignal, SessionReplayCache, SignedSessionEnvelope,
        },
    },
};

#[derive(Clone)]
struct ConnectionContext {
    local_peer_id: PeerId,
    local_identity: LocalPeerIdentity,
    tls_acceptor: tls::Acceptor,
    trusted_peers: Arc<dyn TrustedPeerResolver>,
    limits: Limits,
    incoming_tx: mpsc::Sender<Incoming>,
    request_replay: Arc<Mutex<SessionReplayCache>>,
}

pub(in crate::peer_session) struct Listener {
    port: u16,
    shutdown_tx: watch::Sender<Option<tokio::time::Instant>>,
    failure_rx: watch::Receiver<Option<Error>>,
    task: Option<JoinHandle<Result<(), Error>>>,
    shutdown_timeout: Duration,
}

impl Listener {
    pub(in crate::peer_session) async fn bind(
        port: u16,
        local_peer_id: PeerId,
        local_identity: LocalPeerIdentity,
        trusted_peers: Arc<dyn TrustedPeerResolver>,
        limits: Limits,
        incoming_tx: mpsc::Sender<Incoming>,
    ) -> Result<Self, Error> {
        let listener = bind_dual_stack_listener(port)?;
        let tls_acceptor = tls::Acceptor::new(&local_identity)?;
        let port =
            listener.local_addr().map_err(|error| Error::Listener(error.to_string()))?.port();
        let (shutdown_tx, mut shutdown_rx) = watch::channel(None);
        let semaphore = Arc::new(Semaphore::new(limits.max_connections.max(1)));
        let per_ip = Arc::new(Mutex::new(HashMap::<IpAddr, usize>::new()));
        let request_replay = Arc::new(Mutex::new(SessionReplayCache::new(limits.replay_capacity)));
        let shutdown_timeout = limits.handshake_timeout;
        let (failure_tx, failure_rx) = watch::channel(None);

        let task = tokio::spawn(async move {
            let run_result = AssertUnwindSafe(async move {
                let mut connection_tasks = JoinSet::new();
                let mut authentication_attempts =
                    AuthenticationAttemptLimiter::new(&limits, Instant::now());
                let mut failure = None;
                let mut shutdown_deadline = None;
                loop {
                    tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() {
                            shutdown_deadline = Some(tokio::time::Instant::now());
                            break;
                        }
                        if let Some(deadline) = *shutdown_rx.borrow() {
                            shutdown_deadline = Some(deadline);
                            break;
                        }
                    }
                    joined = connection_tasks.join_next(), if !connection_tasks.is_empty() => {
                        if let Some(Err(error)) = joined {
                            failure = Some(Error::Listener(format!(
                                "signaling connection task failed: {error}"
                            )));
                            break;
                        }
                    }
                    accepted = listener.accept() => {
                        let (stream, address) = match accepted {
                            Ok(accepted) => accepted,
                            Err(_) if shutdown_rx.borrow().is_some() => {
                                shutdown_deadline = *shutdown_rx.borrow();
                                break;
                            }
                            Err(error) => {
                                failure = Some(Error::Listener(format!(
                                    "accept signaling connection: {error}"
                                )));
                                break;
                            }
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
                        let context = ConnectionContext {
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
                };
                connection_tasks.abort_all();
                let drain_deadline = shutdown_deadline
                    .unwrap_or_else(|| tokio::time::Instant::now() + shutdown_timeout)
                    .min(tokio::time::Instant::now() + shutdown_timeout);
                while !connection_tasks.is_empty()
                    && tokio::time::Instant::now() < drain_deadline
                {
                    match tokio::time::timeout_at(drain_deadline, connection_tasks.join_next())
                        .await
                    {
                        Ok(Some(_)) => {}
                        Ok(None) | Err(_) => break,
                    }
                }
                failure.map_or(Ok(()), Err)
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(Error::Listener("signaling listener task panicked".into())));
            if let Err(error) = &run_result {
                failure_tx.send_replace(Some(error.clone()));
            }
            run_result
        });

        Ok(Self { port, shutdown_tx, failure_rx, task: Some(task), shutdown_timeout })
    }

    pub(in crate::peer_session) fn port(&self) -> u16 {
        self.port
    }

    pub(in crate::peer_session) fn failure_receiver(&self) -> watch::Receiver<Option<Error>> {
        self.failure_rx.clone()
    }

    #[cfg(test)]
    pub(in crate::peer_session) async fn shutdown(&mut self) -> Result<(), Error> {
        self.shutdown_until(tokio::time::Instant::now() + self.shutdown_timeout).await
    }

    pub(in crate::peer_session) async fn shutdown_until(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), Error> {
        let Some(task) = self.task.as_mut() else {
            return Ok(());
        };
        let deadline = deadline.min(tokio::time::Instant::now() + self.shutdown_timeout);
        self.shutdown_tx.send_replace(Some(deadline));
        let result = match tokio::time::timeout_at(deadline, &mut *task).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(Error::Listener(format!("listener task join failed: {error}"))),
            Err(_) => {
                task.abort();
                Err(Error::ShutdownTimeout)
            }
        };
        self.task.take();
        result
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shutdown_tx.send_replace(Some(tokio::time::Instant::now()));
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn bind_dual_stack_listener(port: u16) -> Result<TcpListener, Error> {
    let socket = TcpSocket::new_v6()
        .map_err(|error| Error::Listener(format!("create IPv6 socket: {error}")))?;
    SockRef::from(&socket)
        .set_only_v6(false)
        .map_err(|error| Error::Listener(format!("enable dual-stack signaling: {error}")))?;
    socket
        .bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port))
        .map_err(|error| Error::Listener(format!("bind dual-stack socket: {error}")))?;
    socket
        .listen(1024)
        .map_err(|error| Error::Listener(format!("listen on dual-stack socket: {error}")))
}
async fn accept_connection(
    stream: TcpStream,
    context: ConnectionContext,
    permits: ConnectionPermits,
    mut shutdown_rx: watch::Receiver<Option<tokio::time::Instant>>,
) -> Result<(), Error> {
    let ConnectionContext {
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
        .map_err(|error| Error::Signaling(error.to_string()))?;
        let hello = receive_handshake_envelope(&mut socket, &limits).await?;
        let challenge = match hello.payload() {
            NegotiationSignal::EndpointHello { challenge } => *challenge,
            _ => {
                return Err(Error::Protocol(
                    "first signaling message must authenticate the endpoint".into(),
                ));
            }
        };
        let peer_id = hello.from().clone();
        let session_id = hello.session_id();
        let trusted_peer = trusted_peers
            .trusted_peer(&peer_id)
            .await?
            .ok_or_else(|| Error::PeerNotTrusted(peer_id.clone()))?;
        let verification = HandshakeVerification {
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
        let intent = match request.payload() {
            NegotiationSignal::Request {} => Intent::NewSession,
            NegotiationSignal::Restart { generation } => {
                Intent::Restart { generation: *generation }
            }
            _ => {
                return Err(Error::Protocol(
                    "endpoint authentication must be followed by a session intent".into(),
                ));
            }
        };
        verify_incoming_handshake(&request, &verification, &mut replay)?;
        Ok::<_, Error>((socket, session_id, peer_id, trusted_peer, intent, replay))
    };
    let (socket, session_id, peer_id, trusted_peer, intent, replay) =
        tokio::time::timeout_at(authentication_deadline, authenticating)
            .await
            .map_err(|_| Error::Signaling("signaling authentication timed out".into()))??;

    let authenticated_public_key = trusted_peer.public_key.clone();
    let connection = Connection::from_socket(
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
    let routing = incoming_tx.send(Incoming {
        session_id,
        peer_id,
        authenticated_public_key,
        intent,
        connection,
    });
    tokio::pin!(routing);
    tokio::select! {
        changed = shutdown_rx.changed() => {
            let _ = changed;
            Err(Error::ServiceStopped)
        }
        result = tokio::time::timeout_at(authentication_deadline, &mut routing) => {
            result
                .map_err(|_| Error::Signaling("incoming session routing timed out".into()))?
                .map_err(|_| Error::ServiceStopped)
        }
    }
}
struct HandshakeVerification<'a> {
    trusted_peer: &'a TrustedPeerIdentity,
    local_peer_id: &'a PeerId,
    remote_peer_id: &'a PeerId,
    session_id: SessionId,
    limits: &'a Limits,
    shared_replay: &'a Arc<Mutex<SessionReplayCache>>,
}

fn verify_incoming_handshake(
    envelope: &SignedSessionEnvelope,
    context: &HandshakeVerification<'_>,
    connection_replay: &mut SessionReplayCache,
) -> Result<(), Error> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_request_target_is_exact() {
        let request = WebSocketRequest::builder().uri("/session").body(()).unwrap();
        assert!(validate_signaling_request(&request, Response::new(())).is_ok());

        for invalid_target in ["/", "/session/", "/session?unexpected=true"] {
            let request = WebSocketRequest::builder().uri(invalid_target).body(()).unwrap();
            let rejection = validate_signaling_request(&request, Response::new(())).unwrap_err();
            assert_eq!(rejection.status(), StatusCode::NOT_FOUND);
        }
    }
}

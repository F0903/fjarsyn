use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};
use tokio_tungstenite::{WebSocketStream, client_async_with_config, tungstenite::Message};

use super::*;
use crate::{
    identity::{LocalPeerIdentity, PeerId, TrustedPeerIdentity},
    peer_session::{
        Error, NetworkScope, SessionId, TrustedPeerResolver,
        protocol::{NegotiationSignal, SignedSessionEnvelope},
    },
};

#[derive(Debug)]
struct FixedTrustedPeer(TrustedPeerIdentity);

#[async_trait]
impl TrustedPeerResolver for FixedTrustedPeer {
    async fn trusted_peer(&self, peer_id: &PeerId) -> Result<Option<TrustedPeerIdentity>, Error> {
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
    async fn trusted_peer(&self, peer_id: &PeerId) -> Result<Option<TrustedPeerIdentity>, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok((&self.trusted.peer_id == peer_id).then(|| self.trusted.clone()))
    }
}

async fn connect_raw_socket(
    endpoint: SocketAddr,
    trusted_listener: &TrustedPeerIdentity,
    limits: &Limits,
) -> Result<WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>, Error> {
    let connecting = async {
        let stream = TcpStream::connect(endpoint)
            .await
            .map_err(|error| Error::Signaling(error.to_string()))?;
        let stream = tls::Connector::new(trusted_listener)?.connect(endpoint, stream).await?;
        let (socket, _) = client_async_with_config(
            secure_websocket_url(endpoint),
            stream,
            Some(websocket_config(limits)),
        )
        .await
        .map_err(|error| Error::Signaling(error.to_string()))?;
        Ok::<_, Error>(socket)
    };
    tokio::time::timeout(limits.handshake_timeout, connecting)
        .await
        .map_err(|_| Error::Signaling("test WSS handshake timed out".into()))?
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
    let mut listener = Listener::bind(
        0,
        NetworkScope::LoopbackOnly,
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
    assert!(tokio::time::timeout(Duration::from_millis(100), incoming_rx.recv()).await.is_err());

    listener.shutdown().await.unwrap();
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
    let mut listener = Listener::bind(
        0,
        NetworkScope::LoopbackOnly,
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
    assert!(tokio::time::timeout(Duration::from_millis(100), incoming_rx.recv()).await.is_err());
    listener.shutdown().await.unwrap();
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
    let mut listener = Listener::bind(
        0,
        NetworkScope::LoopbackOnly,
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
    let first =
        tokio::time::timeout(Duration::from_secs(1), incoming_rx.recv()).await.unwrap().unwrap();

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
    assert!(tokio::time::timeout(Duration::from_millis(150), incoming_rx.recv()).await.is_err());

    first.connection.shutdown().await;
    let _ = first_socket.close(None).await;
    let _ = replay_socket.close(None).await;
    listener.shutdown().await.unwrap();
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
    let mut listener = Listener::bind(
        0,
        NetworkScope::LoopbackOnly,
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
        let connection = Connection::connect_from_hints(
            &[endpoint],
            NetworkScope::LoopbackOnly,
            SessionConnectionContext {
                session_id,
                local_peer_id: client_peer.clone(),
                remote_peer_id: listener_peer.clone(),
                local_identity: client_identity.clone(),
                trusted_peer: trusted_listener.clone(),
                limits: Limits {
                    max_endpoint_attempts: 1,
                    endpoint_attempt_timeout: Duration::from_secs(1),
                    ..test_limits()
                },
            },
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

    listener.shutdown().await.unwrap();
}

#[tokio::test]
async fn listener_shutdown_cancels_incomplete_handshakes() {
    let identity = LocalPeerIdentity::generate();
    let trusted = Arc::new(FixedTrustedPeer(TrustedPeerIdentity::new(
        PeerId::new("remote").unwrap(),
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let (incoming_tx, _incoming_rx) = mpsc::channel(1);
    let mut listener = Listener::bind(
        0,
        NetworkScope::LoopbackOnly,
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
        .expect("listener shutdown must join handshake tasks")
        .expect("listener shutdown must succeed");
}

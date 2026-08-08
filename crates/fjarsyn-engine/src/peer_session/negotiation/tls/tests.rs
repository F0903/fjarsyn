use std::{net::Ipv4Addr, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::{Acceptor, Connector, SIGNALING_ALPN, require_signaling_alpn};
use crate::identity::{LocalPeerIdentity, PeerId, TrustedPeerIdentity};

fn test_acceptor(
    advertised_identity: &LocalPeerIdentity,
    signing_identity: &LocalPeerIdentity,
    negotiate_alpn: bool,
) -> Acceptor {
    Acceptor::for_test(advertised_identity, signing_identity, negotiate_alpn)
}

async fn read_tls_client_hello(stream: &mut TcpStream) -> Vec<u8> {
    const MAX_RECORD_BYTES: usize = 18 * 1024;
    const MAX_CLIENT_HELLO_BYTES: usize = 64 * 1024;

    let mut observed = Vec::new();
    let mut handshake = Vec::new();
    loop {
        let mut header = [0; 5];
        stream.read_exact(&mut header).await.unwrap();
        assert_eq!(header[0], 0x16, "expected a TLS handshake record");
        let record_len = u16::from_be_bytes([header[3], header[4]]) as usize;
        assert!(record_len <= MAX_RECORD_BYTES);
        let mut payload = vec![0; record_len];
        stream.read_exact(&mut payload).await.unwrap();
        observed.extend_from_slice(&header);
        observed.extend_from_slice(&payload);
        handshake.extend_from_slice(&payload);

        if handshake.len() < 4 {
            continue;
        }
        assert_eq!(handshake[0], 0x01, "expected a TLS ClientHello");
        let handshake_len = ((handshake[1] as usize) << 16)
            | ((handshake[2] as usize) << 8)
            | handshake[3] as usize;
        assert!(handshake_len <= MAX_CLIENT_HELLO_BYTES);
        if handshake.len() >= handshake_len + 4 {
            return observed;
        }
        assert!(observed.len() <= MAX_CLIENT_HELLO_BYTES + MAX_RECORD_BYTES);
    }
}

#[tokio::test]
async fn tls_uses_the_expected_identity_raw_public_key_and_tls13() {
    let server_identity = LocalPeerIdentity::generate();
    let trusted_server = TrustedPeerIdentity::new(
        PeerId::new("server").unwrap(),
        server_identity.public_key_base64(),
    );
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let acceptor = Acceptor::new(&server_identity).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        acceptor.accept(stream).await.unwrap()
    });

    let stream = TcpStream::connect(endpoint).await.unwrap();
    let client = Connector::new(&trusted_server).unwrap().connect(endpoint, stream).await.unwrap();
    assert_eq!(client.get_ref().1.protocol_version(), Some(rustls::ProtocolVersion::TLSv1_3));
    assert_eq!(client.get_ref().1.alpn_protocol(), Some(SIGNALING_ALPN));
    let server = tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap();
    assert_eq!(server.get_ref().1.protocol_version(), Some(rustls::ProtocolVersion::TLSv1_3));
    assert_eq!(server.get_ref().1.alpn_protocol(), Some(SIGNALING_ALPN));
}

#[tokio::test]
async fn tls_rejects_a_server_whose_identity_does_not_match_the_pin() {
    let server_identity = LocalPeerIdentity::generate();
    let wrong_identity = LocalPeerIdentity::generate();
    let trusted_wrong_identity = TrustedPeerIdentity::new(
        PeerId::new("server").unwrap(),
        wrong_identity.public_key_base64(),
    );
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let acceptor = Acceptor::new(&server_identity).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        acceptor.accept(stream).await
    });

    let stream = TcpStream::connect(endpoint).await.unwrap();
    assert!(
        Connector::new(&trusted_wrong_identity).unwrap().connect(endpoint, stream).await.is_err()
    );
    assert!(tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap().is_err());
}

#[tokio::test]
async fn tls_rejects_a_pinned_key_whose_certificate_verify_uses_another_key() {
    let advertised_identity = LocalPeerIdentity::generate();
    let signing_identity = LocalPeerIdentity::generate();
    let trusted_server = TrustedPeerIdentity::new(
        PeerId::new("server").unwrap(),
        advertised_identity.public_key_base64(),
    );
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let acceptor = test_acceptor(&advertised_identity, &signing_identity, true);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        acceptor.accept(stream).await
    });

    let stream = TcpStream::connect(endpoint).await.unwrap();
    assert!(Connector::new(&trusted_server).unwrap().connect(endpoint, stream).await.is_err());
    assert!(tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap().is_err());
}

#[tokio::test]
async fn tls_rejects_a_peer_that_does_not_negotiate_the_signaling_alpn() {
    let server_identity = LocalPeerIdentity::generate();
    let trusted_server = TrustedPeerIdentity::new(
        PeerId::new("server").unwrap(),
        server_identity.public_key_base64(),
    );
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let acceptor = test_acceptor(&server_identity, &server_identity, false);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        acceptor.accept(stream).await
    });

    let stream = TcpStream::connect(endpoint).await.unwrap();
    assert!(Connector::new(&trusted_server).unwrap().connect(endpoint, stream).await.is_err());
    assert!(tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap().is_err());
}

#[tokio::test]
async fn unauthenticated_endpoint_observes_only_a_tls_client_hello() {
    let server_identity = LocalPeerIdentity::generate();
    let trusted_server = TrustedPeerIdentity::new(
        PeerId::new("sensitive-peer-id").unwrap(),
        server_identity.public_key_base64(),
    );
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let observer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_tls_client_hello(&mut stream).await
    });

    let stream = TcpStream::connect(endpoint).await.unwrap();
    assert!(Connector::new(&trusted_server).unwrap().connect(endpoint, stream).await.is_err());
    let observed = tokio::time::timeout(Duration::from_secs(1), observer).await.unwrap().unwrap();
    assert_eq!(observed.first(), Some(&0x16), "expected a TLS handshake record");
    for secret in [b"GET /session".as_slice(), b"EndpointHello", b"sensitive-peer-id"] {
        assert!(
            !observed.windows(secret.len()).any(|window| window == secret),
            "pre-authentication bytes exposed application signaling"
        );
    }
}

#[test]
fn tls_alpn_requires_exact_http_11_negotiation() {
    assert!(require_signaling_alpn(Some(SIGNALING_ALPN)).is_ok());
    assert!(require_signaling_alpn(None).is_err());
    assert!(require_signaling_alpn(Some(b"h2")).is_err());
}

#[tokio::test]
async fn tls_listener_rejects_plaintext_http() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let acceptor = Acceptor::new(&LocalPeerIdentity::generate()).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        acceptor.accept(stream).await
    });

    let mut stream = TcpStream::connect(endpoint).await.unwrap();
    stream.write_all(b"GET /session HTTP/1.1\r\n\r\n").await.unwrap();
    stream.shutdown().await.unwrap();

    assert!(tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap().is_err());
}

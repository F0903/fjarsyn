use std::{fmt, net::SocketAddr, sync::Arc};

use rustls::{
    CertificateError, DigitallySignedStruct, Error as RustlsError, SignatureAlgorithm,
    SignatureScheme,
    client::{
        ClientConfig, Resumption,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::{WebPkiSupportedAlgorithms, verify_tls13_signature_with_raw_key},
    pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime, alg_id},
    server::{AlwaysResolvesServerRawPublicKeys, NoServerSessionStorage, ServerConfig},
    sign::{CertifiedKey, Signer, SigningKey, public_key_to_spki},
};
use tokio::net::TcpStream;
use tokio_rustls::{
    TlsAcceptor, TlsConnector, client::TlsStream as ClientTlsStream,
    server::TlsStream as ServerTlsStream,
};

use super::PeerSessionError;
use crate::identity::{LocalPeerIdentity, TrustedPeerIdentity};

const SIGNALING_ALPN: &[u8] = b"http/1.1";

/// TLS connector whose server raw public key is pinned to one trusted contact.
///
/// A fresh TLS connection is still followed by the signed application
/// challenge/proof. TLS establishes confidentiality and authenticates the
/// listening peer; the signed signaling protocol authenticates the initiator
/// and retains its exact peer/session/replay bindings.
#[derive(Clone)]
pub(super) struct PinnedTlsConnector {
    connector: TlsConnector,
}

impl PinnedTlsConnector {
    pub(super) fn new(trusted_peer: &TrustedPeerIdentity) -> Result<Self, PeerSessionError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifying_key = trusted_peer
            .verifying_key()
            .map_err(|error| PeerSessionError::Protocol(error.to_string()))?;
        let expected_spki = identity_spki(verifying_key.as_bytes());
        let verifier = Arc::new(PinnedServerIdentity {
            expected_spki,
            algorithms: provider.signature_verification_algorithms,
        });
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|error| tls_protocol_error("configure TLS 1.3 client", error))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        config.resumption = Resumption::disabled();
        config.enable_early_data = false;
        config.enable_sni = false;
        config.alpn_protocols = vec![SIGNALING_ALPN.to_vec()];

        Ok(Self { connector: TlsConnector::from(Arc::new(config)) })
    }

    pub(super) async fn connect(
        &self,
        endpoint: SocketAddr,
        stream: TcpStream,
    ) -> Result<ClientTlsStream<TcpStream>, PeerSessionError> {
        let stream = self
            .connector
            .connect(ServerName::from(endpoint.ip()), stream)
            .await
            .map_err(|error| tls_signaling_error("TLS peer authentication failed", error))?;
        require_signaling_alpn(stream.get_ref().1.alpn_protocol())?;
        Ok(stream)
    }
}

#[derive(Clone)]
pub(super) struct IdentityTlsAcceptor {
    acceptor: TlsAcceptor,
}

impl IdentityTlsAcceptor {
    pub(super) fn new(local_identity: &LocalPeerIdentity) -> Result<Self, PeerSessionError> {
        let signing_key: Arc<dyn SigningKey> =
            Arc::new(IdentitySigningKey { identity: local_identity.clone() });
        let raw_public_key = signing_key
            .public_key()
            .ok_or_else(|| PeerSessionError::Listener("TLS identity has no public key".into()))?;
        let certified_key = Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(raw_public_key.as_ref().to_vec())],
            signing_key,
        ));
        let config = signaling_server_config(certified_key)?;

        Ok(Self { acceptor: TlsAcceptor::from(Arc::new(config)) })
    }

    pub(super) async fn accept(
        &self,
        stream: TcpStream,
    ) -> Result<ServerTlsStream<TcpStream>, PeerSessionError> {
        let stream = self
            .acceptor
            .accept(stream)
            .await
            .map_err(|error| tls_signaling_error("TLS signaling handshake failed", error))?;
        require_signaling_alpn(stream.get_ref().1.alpn_protocol())?;
        Ok(stream)
    }
}

fn signaling_server_config(
    certified_key: Arc<CertifiedKey>,
) -> Result<ServerConfig, PeerSessionError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let resolver = Arc::new(AlwaysResolvesServerRawPublicKeys::new(certified_key));
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| tls_listener_error("configure TLS 1.3 server", error))?
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    config.session_storage = Arc::new(NoServerSessionStorage {});
    config.send_tls13_tickets = 0;
    config.max_early_data_size = 0;
    config.send_half_rtt_data = false;
    config.alpn_protocols = vec![SIGNALING_ALPN.to_vec()];
    Ok(config)
}

fn identity_spki(public_key: &[u8; 32]) -> SubjectPublicKeyInfoDer<'static> {
    public_key_to_spki(&alg_id::ED25519, public_key)
}

#[derive(Clone)]
struct IdentitySigningKey {
    identity: LocalPeerIdentity,
}

impl fmt::Debug for IdentitySigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IdentitySigningKey(<redacted>)")
    }
}

impl SigningKey for IdentitySigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered.contains(&SignatureScheme::ED25519).then(|| {
            Box::new(IdentitySigner { identity: self.identity.clone() }) as Box<dyn Signer>
        })
    }

    fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
        Some(identity_spki(self.identity.verifying_key().as_bytes()))
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ED25519
    }
}

struct IdentitySigner {
    identity: LocalPeerIdentity,
}

impl fmt::Debug for IdentitySigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IdentitySigner(<redacted>)")
    }
}

impl Signer for IdentitySigner {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, RustlsError> {
        Ok(self.identity.sign(message).to_bytes().to_vec())
    }

    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ED25519
    }
}

#[derive(Debug)]
struct PinnedServerIdentity {
    expected_spki: SubjectPublicKeyInfoDer<'static>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedServerIdentity {
    fn verify_server_cert(
        &self,
        raw_public_key: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if !intermediates.is_empty()
            || !ocsp_response.is_empty()
            || raw_public_key.as_ref() != self.expected_spki.as_ref()
        {
            return Err(CertificateError::ApplicationVerificationFailure.into());
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _raw_public_key: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Err(RustlsError::General("TLS 1.2 is disabled for signaling".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        raw_public_key: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        if raw_public_key.as_ref() != self.expected_spki.as_ref() {
            return Err(CertificateError::ApplicationVerificationFailure.into());
        }
        verify_tls13_signature_with_raw_key(
            message,
            &self.expected_spki,
            signature,
            &self.algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

fn tls_protocol_error(context: &str, error: impl fmt::Display) -> PeerSessionError {
    PeerSessionError::Protocol(format!("{context}: {error}"))
}

fn tls_listener_error(context: &str, error: impl fmt::Display) -> PeerSessionError {
    PeerSessionError::Listener(format!("{context}: {error}"))
}

fn tls_signaling_error(context: &str, error: impl fmt::Display) -> PeerSessionError {
    PeerSessionError::Signaling(format!("{context}: {error}"))
}

fn require_signaling_alpn(selected: Option<&[u8]>) -> Result<(), PeerSessionError> {
    if selected == Some(SIGNALING_ALPN) {
        Ok(())
    } else {
        Err(PeerSessionError::Signaling(
            "TLS peer did not negotiate the required HTTP/1.1 signaling protocol".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, time::Duration};

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;
    use crate::identity::PeerId;

    fn test_acceptor(
        advertised_identity: &LocalPeerIdentity,
        signing_identity: &LocalPeerIdentity,
        negotiate_alpn: bool,
    ) -> IdentityTlsAcceptor {
        let signing_key: Arc<dyn SigningKey> =
            Arc::new(IdentitySigningKey { identity: signing_identity.clone() });
        let advertised_spki = identity_spki(advertised_identity.verifying_key().as_bytes());
        let certified_key = Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(advertised_spki.as_ref().to_vec())],
            signing_key,
        ));
        let mut config = signaling_server_config(certified_key).unwrap();
        if !negotiate_alpn {
            config.alpn_protocols.clear();
        }
        IdentityTlsAcceptor { acceptor: TlsAcceptor::from(Arc::new(config)) }
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
        let acceptor = IdentityTlsAcceptor::new(&server_identity).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            acceptor.accept(stream).await.unwrap()
        });

        let stream = TcpStream::connect(endpoint).await.unwrap();
        let client = PinnedTlsConnector::new(&trusted_server)
            .unwrap()
            .connect(endpoint, stream)
            .await
            .unwrap();
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
        let acceptor = IdentityTlsAcceptor::new(&server_identity).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            acceptor.accept(stream).await
        });

        let stream = TcpStream::connect(endpoint).await.unwrap();
        assert!(
            PinnedTlsConnector::new(&trusted_wrong_identity)
                .unwrap()
                .connect(endpoint, stream)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap().is_err()
        );
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
        assert!(
            PinnedTlsConnector::new(&trusted_server)
                .unwrap()
                .connect(endpoint, stream)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap().is_err()
        );
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
        assert!(
            PinnedTlsConnector::new(&trusted_server)
                .unwrap()
                .connect(endpoint, stream)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap().is_err()
        );
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
        assert!(
            PinnedTlsConnector::new(&trusted_server)
                .unwrap()
                .connect(endpoint, stream)
                .await
                .is_err()
        );
        let observed =
            tokio::time::timeout(Duration::from_secs(1), observer).await.unwrap().unwrap();
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
        let acceptor = IdentityTlsAcceptor::new(&LocalPeerIdentity::generate()).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            acceptor.accept(stream).await
        });

        let mut stream = TcpStream::connect(endpoint).await.unwrap();
        stream.write_all(b"GET /session HTTP/1.1\r\n\r\n").await.unwrap();
        stream.shutdown().await.unwrap();

        assert!(
            tokio::time::timeout(Duration::from_secs(1), server).await.unwrap().unwrap().is_err()
        );
    }
}

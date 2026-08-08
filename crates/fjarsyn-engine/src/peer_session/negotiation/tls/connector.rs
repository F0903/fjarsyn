use std::{net::SocketAddr, sync::Arc};

use rustls::{
    CertificateError, DigitallySignedStruct, Error as RustlsError, SignatureScheme,
    client::{
        ClientConfig, Resumption,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::{WebPkiSupportedAlgorithms, verify_tls13_signature_with_raw_key},
    pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime},
};
use tokio::net::TcpStream;
use tokio_rustls::{TlsConnector, client::TlsStream};

use super::{
    SIGNALING_ALPN, identity_spki, protocol_error, require_signaling_alpn, signaling_error,
};
use crate::{identity::TrustedPeerIdentity, peer_session::Error};

/// TLS connector whose server raw public key is pinned to one trusted contact.
///
/// A fresh TLS connection is still followed by the signed application
/// challenge/proof. TLS establishes confidentiality and authenticates the
/// listening peer; the signed signaling protocol authenticates the initiator
/// and retains its exact peer/session/replay bindings.
#[derive(Clone)]
pub(in crate::peer_session::negotiation) struct Connector {
    connector: TlsConnector,
}

impl Connector {
    pub(in crate::peer_session::negotiation) fn new(
        trusted_peer: &TrustedPeerIdentity,
    ) -> Result<Self, Error> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifying_key =
            trusted_peer.verifying_key().map_err(|error| Error::Protocol(error.to_string()))?;
        let expected_spki = identity_spki(verifying_key.as_bytes());
        let verifier = Arc::new(PinnedServerIdentity {
            expected_spki,
            algorithms: provider.signature_verification_algorithms,
        });
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|error| protocol_error("configure TLS 1.3 client", error))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        config.resumption = Resumption::disabled();
        config.enable_early_data = false;
        config.enable_sni = false;
        config.alpn_protocols = vec![SIGNALING_ALPN.to_vec()];

        Ok(Self { connector: TlsConnector::from(Arc::new(config)) })
    }

    pub(in crate::peer_session::negotiation) async fn connect(
        &self,
        endpoint: SocketAddr,
        stream: TcpStream,
    ) -> Result<TlsStream<TcpStream>, Error> {
        let stream = self
            .connector
            .connect(ServerName::from(endpoint.ip()), stream)
            .await
            .map_err(|error| signaling_error("TLS peer authentication failed", error))?;
        require_signaling_alpn(stream.get_ref().1.alpn_protocol())?;
        Ok(stream)
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

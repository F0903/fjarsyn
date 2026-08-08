use std::{fmt, sync::Arc};

use rustls::{
    Error as RustlsError, SignatureAlgorithm, SignatureScheme,
    pki_types::{CertificateDer, SubjectPublicKeyInfoDer},
    server::{AlwaysResolvesServerRawPublicKeys, NoServerSessionStorage, ServerConfig},
    sign::{CertifiedKey, Signer, SigningKey},
};
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, server::TlsStream};

use super::{
    SIGNALING_ALPN, identity_spki, listener_error, require_signaling_alpn, signaling_error,
};
use crate::{identity::LocalPeerIdentity, peer_session::Error};

#[derive(Clone)]
pub(in crate::peer_session::negotiation) struct Acceptor {
    acceptor: TlsAcceptor,
}

impl Acceptor {
    pub(in crate::peer_session::negotiation) fn new(
        local_identity: &LocalPeerIdentity,
    ) -> Result<Self, Error> {
        Self::with_identities(local_identity, local_identity, true)
    }

    pub(in crate::peer_session::negotiation) async fn accept(
        &self,
        stream: TcpStream,
    ) -> Result<TlsStream<TcpStream>, Error> {
        let stream = self
            .acceptor
            .accept(stream)
            .await
            .map_err(|error| signaling_error("TLS signaling handshake failed", error))?;
        require_signaling_alpn(stream.get_ref().1.alpn_protocol())?;
        Ok(stream)
    }

    fn with_identities(
        advertised_identity: &LocalPeerIdentity,
        signing_identity: &LocalPeerIdentity,
        negotiate_alpn: bool,
    ) -> Result<Self, Error> {
        let signing_key: Arc<dyn SigningKey> =
            Arc::new(IdentitySigningKey { identity: signing_identity.clone() });
        let advertised_spki = identity_spki(advertised_identity.verifying_key().as_bytes());
        let certified_key = Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(advertised_spki.as_ref().to_vec())],
            signing_key,
        ));
        let mut config = signaling_server_config(certified_key)?;
        if !negotiate_alpn {
            config.alpn_protocols.clear();
        }

        Ok(Self { acceptor: TlsAcceptor::from(Arc::new(config)) })
    }

    #[cfg(test)]
    pub(super) fn for_test(
        advertised_identity: &LocalPeerIdentity,
        signing_identity: &LocalPeerIdentity,
        negotiate_alpn: bool,
    ) -> Self {
        Self::with_identities(advertised_identity, signing_identity, negotiate_alpn).unwrap()
    }
}

fn signaling_server_config(certified_key: Arc<CertifiedKey>) -> Result<ServerConfig, Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let resolver = Arc::new(AlwaysResolvesServerRawPublicKeys::new(certified_key));
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| listener_error("configure TLS 1.3 server", error))?
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    config.session_storage = Arc::new(NoServerSessionStorage {});
    config.send_tls13_tickets = 0;
    config.max_early_data_size = 0;
    config.send_half_rtt_data = false;
    config.alpn_protocols = vec![SIGNALING_ALPN.to_vec()];
    Ok(config)
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

//! Identity-bound TLS endpoints for authenticated signaling.

use std::fmt;

use rustls::{
    pki_types::{SubjectPublicKeyInfoDer, alg_id},
    sign::public_key_to_spki,
};

use crate::peer_session::Error;

mod acceptor;
mod connector;

const SIGNALING_ALPN: &[u8] = b"http/1.1";

fn identity_spki(public_key: &[u8; 32]) -> SubjectPublicKeyInfoDer<'static> {
    public_key_to_spki(&alg_id::ED25519, public_key)
}

fn protocol_error(context: &str, error: impl fmt::Display) -> Error {
    Error::Protocol(format!("{context}: {error}"))
}

fn listener_error(context: &str, error: impl fmt::Display) -> Error {
    Error::Listener(format!("{context}: {error}"))
}

fn signaling_error(context: &str, error: impl fmt::Display) -> Error {
    Error::Signaling(format!("{context}: {error}"))
}

fn require_signaling_alpn(selected: Option<&[u8]>) -> Result<(), Error> {
    if selected == Some(SIGNALING_ALPN) {
        Ok(())
    } else {
        Err(Error::Signaling(
            "TLS peer did not negotiate the required HTTP/1.1 signaling protocol".into(),
        ))
    }
}

pub(super) use acceptor::Acceptor;
pub(super) use connector::Connector;

#[cfg(test)]
mod tests;

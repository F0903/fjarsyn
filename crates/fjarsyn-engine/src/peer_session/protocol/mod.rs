//! Signed signaling and ordered data-channel protocol messages.

mod control_message;
mod messaging_message;
mod negotiation_signal;
mod session_replay_cache;
mod signed_session_envelope;

use crate::peer_session::Error;

const SIGNALING_VERSION: u8 = 1;
pub(in crate::peer_session) const DATA_PROTOCOL_VERSION: u8 = 2;

fn validate_data_version(version: u8) -> Result<(), Error> {
    if version == DATA_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(Error::Protocol(format!("unsupported data protocol version {version}")))
    }
}

#[cfg(test)]
mod tests;

pub(in crate::peer_session) use control_message::ControlMessage;
pub(in crate::peer_session) use messaging_message::MessagingMessage;
pub(in crate::peer_session) use negotiation_signal::NegotiationSignal;
pub(in crate::peer_session) use session_replay_cache::SessionReplayCache;
pub(in crate::peer_session) use signed_session_envelope::{
    EnvelopeVerification, SignedSessionEnvelope,
};

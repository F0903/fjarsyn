//! Long-lived peer identities used to authenticate temporary session signaling.

mod error;
mod key_encoding;
mod local_peer_identity;
mod peer_id;
mod trusted_peer_identity;

pub use error::Error;
pub use local_peer_identity::{LocalPeerIdentity, StoredIdentityKeypair};
pub use peer_id::{PeerId, PeerIdError};
pub use trusted_peer_identity::TrustedPeerIdentity;

#[cfg(test)]
mod tests;

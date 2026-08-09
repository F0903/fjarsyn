//! Long-lived peer identities used to authenticate temporary session signaling.

mod error;
mod key_encoding;
mod local_identity;
mod local_peer_identity;
mod peer_id;
#[cfg(target_os = "windows")]
mod protection;
mod store;
mod trusted_peer_identity;

pub use error::Error;
pub use local_identity::LocalIdentity;
pub(crate) use local_peer_identity::{LocalPeerIdentity, StoredIdentityKeypair};
pub use peer_id::{PeerId, PeerIdError};
pub(crate) use store::Store;
pub use trusted_peer_identity::TrustedPeerIdentity;

#[cfg(test)]
mod tests;

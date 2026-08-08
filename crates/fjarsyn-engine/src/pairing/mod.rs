//! Explicit, human-confirmed exchange of long-lived peer identities.
//!
//! Pairing invites are transport-neutral text. Parsing an invite validates its
//! canonical representation and cryptographic key, while [`Invite::confirm`]
//! records the separate semantic step in which a user confirmed the fingerprint.

mod identity_fingerprint;
mod invite;
mod verified_peer_identity;

pub use identity_fingerprint::IdentityFingerprint;
pub use invite::{Error, Invite, MAX_INVITE_BYTES};
pub use verified_peer_identity::VerifiedPeerIdentity;

#[cfg(test)]
mod tests;

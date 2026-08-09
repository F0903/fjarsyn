//! Stable application-facing state and identifiers for peer sessions.

mod close_reason;
mod event;
mod id;
mod phase;
mod sessions;
mod share_epoch;
mod share_state;
mod transport_generation;

pub use close_reason::CloseReason;
pub use event::Event;
pub use id::{MessageId, SessionId, ShareId};
pub use phase::Phase;
pub use sessions::{SessionState, Sessions};
pub use share_epoch::ShareEpoch;
pub use share_state::{LocalShareState, RemoteShareState};
pub(in crate::peer_session) use transport_generation::TransportGeneration;

#[cfg(test)]
mod tests;

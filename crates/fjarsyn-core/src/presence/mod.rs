//! Local peer presence through mDNS.
//!
//! Presence is deliberately limited to discovery and reachability hints. It
//! neither establishes trust nor owns peer connections; callers must
//! authenticate the expected peer while establishing a session.

mod mdns;
mod model;
mod service;

pub use model::{NearbyAdvertisement, NearbyPeer, PresenceLimits, PresenceSnapshot};
pub use service::{PresenceHandle, PresenceService, PresenceServiceConfig, PresenceServiceError};

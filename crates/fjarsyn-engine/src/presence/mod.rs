//! Local peer presence through mDNS.
//!
//! Presence is deliberately limited to discovery and reachability hints. It
//! neither establishes trust nor owns peer connections; callers must
//! authenticate the expected peer while establishing a session.

mod config;
mod discovery;
mod error;
mod nearby_peer;
mod presence_service;
mod service_handle;
mod snapshot;

pub use config::{Config, Limits};
use discovery::{Backend, MdnsBackend, Observation, Registry, ResolvedAdvertisement};
pub use error::Error;
pub use nearby_peer::{NearbyAdvertisement, NearbyPeer};
pub use presence_service::PresenceService;
pub use service_handle::ServiceHandle;
pub use snapshot::Snapshot;

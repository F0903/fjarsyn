//! Local peer presence through mDNS.
//!
//! Presence is deliberately limited to discovery and reachability hints. It
//! neither establishes trust nor owns peer connections; callers must
//! authenticate the expected peer while establishing a session.
//! [`crate::Engine`] owns its hosted implementation and publishes only the
//! typed [`ServiceHandle`] plus presence-domain values.

mod config;
mod discovery;
mod error;
mod nearby_peer;
mod nearby_peers;
mod presence_service;
mod service_handle;

pub(crate) use config::Config;
use config::Limits;
use discovery::{Backend, MdnsBackend, Observation, Registry, ResolvedAdvertisement};
pub(crate) use error::Error;
pub use nearby_peer::{NearbyAdvertisement, NearbyPeer};
pub use nearby_peers::NearbyPeers;
pub(crate) use presence_service::PresenceService;
pub use service_handle::ServiceHandle;

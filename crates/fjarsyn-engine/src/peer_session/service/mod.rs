//! Application-facing service API and its orchestration runtime.

mod config;
mod endpoint_resolver;
mod limits;
mod orchestration;
mod peer_session_service;
mod service_handle;
mod trusted_peer_resolver;

#[cfg(test)]
mod tests;

pub use config::Config;
pub use endpoint_resolver::EndpointResolver;
pub use limits::Limits;
pub(crate) use orchestration::TrustBarrierOwnerId;
pub use peer_session_service::PeerSessionService;
pub use service_handle::ServiceHandle;
pub use trusted_peer_resolver::TrustedPeerResolver;

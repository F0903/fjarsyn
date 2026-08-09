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

pub(crate) use config::Config;
pub(crate) use endpoint_resolver::EndpointResolver;
pub(crate) use limits::Limits;
pub(crate) use orchestration::TrustBarrierOwnerId;
pub(crate) use peer_session_service::PeerSessionService;
pub use service_handle::ServiceHandle;
pub(crate) use trusted_peer_resolver::TrustedPeerResolver;

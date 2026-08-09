//! Deliberate, authenticated WebRTC peer sessions.
//!
//! This feature owns the complete lifetime of a peer connection. Application
//! code interacts through [`ServiceHandle`]; screen-share media orchestration
//! stays behind the engine's `screen_share` capability, while WebRTC and
//! signaling transport objects never leave this module.

mod actor;
mod error;
mod media;
mod model;
mod negotiation;
mod network_scope;
mod protocol;
mod restart;
mod rtc;
mod service;

pub use error::Error;
pub(crate) use media::{EncodedVideoSample, EncodedVideoSink, RemoteVideoRead, RemoteVideoSource};
pub(in crate::peer_session) use model::TransportGeneration;
pub use model::{
    CloseReason, Event, LocalShareState, MessageId, Phase, RemoteShareState, SessionId,
    SessionState, Sessions, ShareEpoch, ShareId,
};
pub(crate) use network_scope::NetworkScope;
pub use service::ServiceHandle;
pub(crate) use service::{
    Config, EndpointResolver, Limits, PeerSessionService, TrustBarrierOwnerId, TrustedPeerResolver,
};

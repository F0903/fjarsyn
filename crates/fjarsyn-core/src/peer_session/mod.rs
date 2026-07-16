//! Deliberate, authenticated WebRTC peer sessions.
//!
//! This feature owns the complete lifetime of a peer connection. Application
//! code interacts through [`PeerSessionServiceHandle`] and bounded media
//! capabilities; WebRTC and signaling transport objects never leave the
//! module.

mod actor;
mod error;
mod media;
mod model;
mod negotiation;
mod protocol;
mod rtc;
mod service;
mod state_machine;

pub use error::PeerSessionError;
pub use media::{EncodedVideoSample, EncodedVideoSink, RemoteVideoSource};
pub use model::{
    LocalShareState, MessageId, PeerSessionEvent, PeerSessionPhase, PeerSessionServiceSnapshot,
    PeerSessionSnapshot, RemoteShareState, SessionCloseReason, SessionId, ShareId,
};
pub(crate) use service::TrustBarrierOwnerId;
pub use service::{
    PeerEndpointResolver, PeerSessionLimits, PeerSessionService, PeerSessionServiceConfig,
    PeerSessionServiceHandle, TrustedPeerResolver,
};

pub use crate::identity::PeerId;

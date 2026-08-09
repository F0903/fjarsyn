//! Supervised video-codec execution.
//!
//! Every codec instance is constructed, called, and destroyed on one dedicated
//! OS thread. The async side applies bounded call/stop deadlines and permanently
//! disables only the affected direction when a deadline is missed. Rust cannot
//! safely terminate a native call on another thread: a timed-out thread is
//! quarantined until it returns or the process exits, and Fjarsyn must restart
//! before that codec direction can be used again.
//! The current FFmpeg implementation is a private backend of this boundary.
//!
//! Files follow runtime ownership boundaries: small commands, parts, and
//! diagnostics stay with the object that owns them, while the substantial
//! encoder, decoder, registry, and worker lifecycles remain distinct.

use std::time::Duration;

const REAPER_POLL_INTERVAL: Duration = Duration::from_millis(25);

mod backend;
mod codec_service;
mod decoder;
mod encoder;
mod error;
mod health;
mod registry;
mod service_handle;
mod state;
mod transcode;
mod worker;

#[cfg(test)]
mod tests;

#[cfg(test)]
use codec_service::ShutdownError;
pub(crate) use codec_service::{CodecService, Config};
pub use decoder::{
    DecoderInput, DecoderOutput, DecoderSession, DecoderSessionParts, DecoderWorkerConfig,
};
pub use encoder::{
    EncodedFrame, EncoderInput, EncoderOutput, EncoderSession, EncoderSessionParts,
    EncoderWorkerConfig,
};
pub use error::Error;
pub use health::{Direction, DirectionState, Health, Operation, Poison, PoisonReason};
pub use service_handle::ServiceHandle;
pub(in crate::media::codec) use state::State;
pub use transcode::TranscodeType;
pub use worker::{Worker, WorkerError};

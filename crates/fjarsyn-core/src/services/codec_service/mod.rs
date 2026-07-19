//! Application-owned FFmpeg worker service.
//!
//! Every codec instance is constructed, called, and destroyed on one dedicated
//! OS thread. The async side applies bounded call/stop deadlines and permanently
//! disables only the affected direction when a deadline is missed. Rust cannot
//! safely terminate a native call on another thread: a timed-out thread is
//! quarantined until it returns or the process exits, and Fjarsyn must restart
//! before that codec direction can be used again.
//!
//! Each independently meaningful object lives in its own same-named source
//! file. The backend, registry, encoder, decoder, and worker directories make
//! the runtime ownership tree visible without opening an implementation file.

mod backend;
mod codec_direction;
mod codec_direction_state;
mod codec_operation;
mod codec_poison;
mod codec_poison_reason;
mod config;
mod decoder;
mod encoder;
mod error;
mod handle;
mod registry;
mod service;
mod shutdown_error;
mod snapshot;
mod state;
mod worker;

#[cfg(test)]
mod tests;

pub use codec_direction::CodecDirection;
pub use codec_direction_state::CodecDirectionState;
pub use codec_operation::CodecOperation;
pub use codec_poison::CodecPoison;
pub use codec_poison_reason::CodecPoisonReason;
pub use config::Config;
pub use decoder::{
    DecoderInput, DecoderOutput, DecoderSession, DecoderSessionParts, DecoderWorkerConfig,
};
pub use encoder::{
    EncodedFrame, EncoderInput, EncoderOutput, EncoderSession, EncoderSessionParts,
    EncoderWorkerConfig,
};
pub use error::Error;
pub use handle::Handle;
pub use service::Service;
pub use shutdown_error::ShutdownError;
pub use snapshot::Snapshot;
pub(in crate::services::codec_service) use state::State;
pub use worker::{CodecWorker, CodecWorkerError};

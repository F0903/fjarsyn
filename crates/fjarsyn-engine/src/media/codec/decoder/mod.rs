//! Decoder session channels, supervision, and dedicated worker-thread ownership.

use crate::media::{PixelFormat, codec::TranscodeType};

mod session;
mod supervisor;
mod thread;

#[derive(Debug, Clone, Copy)]
pub struct DecoderWorkerConfig {
    pub transcoding_type: TranscodeType,
    pub output_format: PixelFormat,
}

pub use session::{DecoderInput, DecoderOutput, DecoderSession, DecoderSessionParts};
pub(in crate::media::codec) use supervisor::Supervisor;
pub(in crate::media::codec) use thread::{Command, Thread};

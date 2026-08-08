//! Encoder session channels, supervision, and dedicated worker-thread ownership.

use crate::media::{CodecDeviceLease, PixelFormat, codec::TranscodeType, video::TargetResolution};

mod session;
mod supervisor;
mod thread;

#[derive(Debug, Clone)]
pub struct EncoderWorkerConfig {
    pub bitrate: u32,
    pub target_framerate_hz: f32,
    pub target_resolution: TargetResolution,
    pub input_format: PixelFormat,
    pub device: Option<CodecDeviceLease>,
    pub transcoding_type: TranscodeType,
}

pub use session::{EncodedFrame, EncoderInput, EncoderOutput, EncoderSession, EncoderSessionParts};
pub(in crate::media::codec) use supervisor::Supervisor;
pub(in crate::media::codec) use thread::{Command, Thread};

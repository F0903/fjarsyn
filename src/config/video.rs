use serde::{Deserialize, Serialize};

use crate::{
    capture_providers::CaptureFramerate,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoConfig {
    pub target_bitrate: u32,
    pub target_framerate: CaptureFramerate,
    pub target_resolution: TargetResolution,
    pub transcoding_type: FFmpegTranscodeType,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            target_bitrate: 8_000_000,
            target_framerate: CaptureFramerate::FPS60,
            target_resolution: TargetResolution::Source,
            transcoding_type: FFmpegTranscodeType::default(),
        }
    }
}

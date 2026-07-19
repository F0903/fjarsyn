use crate::media::{
    CodecDeviceLease, FFmpegTranscodeType, TargetResolution, pixel_format::PixelFormat,
};

#[derive(Debug, Clone)]
pub struct EncoderWorkerConfig {
    pub bitrate: u32,
    pub target_framerate_hz: f32,
    pub target_resolution: TargetResolution,
    pub input_format: PixelFormat,
    pub device: Option<CodecDeviceLease>,
    pub transcoding_type: FFmpegTranscodeType,
}

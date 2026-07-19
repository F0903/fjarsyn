use crate::media::{FFmpegTranscodeType, pixel_format::PixelFormat};

#[derive(Debug, Clone, Copy)]
pub struct DecoderWorkerConfig {
    pub transcoding_type: FFmpegTranscodeType,
    pub output_format: PixelFormat,
}

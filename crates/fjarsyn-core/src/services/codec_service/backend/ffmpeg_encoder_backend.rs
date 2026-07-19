use super::EncoderBackend;
use crate::media::{FFmpegTranscodeType, ffmpeg::FFmpegEncoder, frame::Frame};

pub(super) struct FfmpegEncoderBackend {
    encoder: FFmpegEncoder,
    transcoding_type: FFmpegTranscodeType,
}

impl FfmpegEncoderBackend {
    pub(super) fn new(encoder: FFmpegEncoder, transcoding_type: FFmpegTranscodeType) -> Self {
        Self { encoder, transcoding_type }
    }
}

impl EncoderBackend for FfmpegEncoderBackend {
    fn encode(&mut self, frame: &Frame) -> Result<Vec<Vec<u8>>, String> {
        self.encoder
            .encode(frame, self.transcoding_type, frame.size.x, frame.size.y)
            .map_err(|error| error.to_string())
    }
}

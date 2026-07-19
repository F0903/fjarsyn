use std::sync::Arc;

use super::DecoderBackend;
use crate::media::{ffmpeg::FFmpegDecoder, frame::Frame};

pub(super) struct FfmpegDecoderBackend {
    decoder: FFmpegDecoder,
}

impl FfmpegDecoderBackend {
    pub(super) fn new(decoder: FFmpegDecoder) -> Self {
        Self { decoder }
    }
}

impl DecoderBackend for FfmpegDecoderBackend {
    fn decode(&mut self, packet: &[u8]) -> Result<Option<Arc<Frame>>, String> {
        self.decoder.decode(packet).map_err(|error| error.to_string())
    }
}

//! Injectable codec backends and their FFmpeg implementation.

use std::sync::Arc;

use crate::media::{
    codec::{DecoderWorkerConfig, EncoderWorkerConfig, TranscodeType},
    frame::Frame,
};

mod ffmpeg;

use ffmpeg::{Decoder, Encoder};

pub(in crate::media::codec) trait DecoderBackend {
    fn decode(&mut self, packet: &[u8]) -> Result<Option<Arc<Frame>>, String>;
}

pub(in crate::media::codec) trait EncoderBackend {
    fn encode(&mut self, frame: &Frame, force_keyframe: bool) -> Result<Vec<Vec<u8>>, String>;
}

pub(in crate::media::codec) trait BackendFactory:
    Send + Sync + 'static
{
    fn create_encoder(
        &self,
        config: EncoderWorkerConfig,
    ) -> Result<Box<dyn EncoderBackend>, String>;

    fn create_decoder(
        &self,
        config: DecoderWorkerConfig,
    ) -> Result<Box<dyn DecoderBackend>, String>;
}

pub(in crate::media::codec) struct FfmpegBackendFactory;

impl BackendFactory for FfmpegBackendFactory {
    fn create_encoder(
        &self,
        config: EncoderWorkerConfig,
    ) -> Result<Box<dyn EncoderBackend>, String> {
        let encoder = Encoder::new(
            config.bitrate,
            config.target_framerate_hz,
            config.target_resolution,
            config.device,
            config.transcoding_type,
        )
        .map_err(|error| error.to_string())?;
        Ok(Box::new(FfmpegEncoderBackend { encoder, transcoding_type: config.transcoding_type }))
    }

    fn create_decoder(
        &self,
        config: DecoderWorkerConfig,
    ) -> Result<Box<dyn DecoderBackend>, String> {
        Decoder::new(config.transcoding_type, config.output_format)
            .map(|decoder| Box::new(FfmpegDecoderBackend { decoder }) as Box<dyn DecoderBackend>)
            .map_err(|error| error.to_string())
    }
}

struct FfmpegDecoderBackend {
    decoder: Decoder,
}

impl DecoderBackend for FfmpegDecoderBackend {
    fn decode(&mut self, packet: &[u8]) -> Result<Option<Arc<Frame>>, String> {
        self.decoder.decode(packet).map_err(|error| error.to_string())
    }
}

struct FfmpegEncoderBackend {
    encoder: Encoder,
    transcoding_type: TranscodeType,
}

impl EncoderBackend for FfmpegEncoderBackend {
    fn encode(&mut self, frame: &Frame, force_keyframe: bool) -> Result<Vec<Vec<u8>>, String> {
        self.encoder
            .encode(frame, self.transcoding_type, force_keyframe)
            .map_err(|error| error.to_string())
    }
}

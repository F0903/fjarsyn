use super::{
    CodecBackendFactory, DecoderBackend, EncoderBackend,
    ffmpeg_decoder_backend::FfmpegDecoderBackend, ffmpeg_encoder_backend::FfmpegEncoderBackend,
};
use crate::{
    media::ffmpeg::{FFmpegDecoder, FFmpegEncoder},
    services::codec_service::{DecoderWorkerConfig, EncoderWorkerConfig},
};

pub(in crate::services::codec_service) struct FfmpegCodecBackendFactory;

impl CodecBackendFactory for FfmpegCodecBackendFactory {
    fn create_encoder(
        &self,
        config: EncoderWorkerConfig,
    ) -> Result<Box<dyn EncoderBackend>, String> {
        let encoder = FFmpegEncoder::new(
            config.bitrate,
            config.target_framerate_hz,
            config.target_resolution,
            config.input_format,
            config.device,
            config.transcoding_type,
        )
        .map_err(|error| error.to_string())?;
        Ok(Box::new(FfmpegEncoderBackend::new(encoder, config.transcoding_type)))
    }

    fn create_decoder(
        &self,
        config: DecoderWorkerConfig,
    ) -> Result<Box<dyn DecoderBackend>, String> {
        FFmpegDecoder::new(config.transcoding_type, config.output_format)
            .map(FfmpegDecoderBackend::new)
            .map(|decoder| Box::new(decoder) as Box<dyn DecoderBackend>)
            .map_err(|error| error.to_string())
    }
}

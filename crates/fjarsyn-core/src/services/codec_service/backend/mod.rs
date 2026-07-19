//! Injectable codec backends and their FFmpeg implementations.

mod codec_backend_factory;
mod decoder_backend;
mod encoder_backend;
mod ffmpeg_codec_backend_factory;
mod ffmpeg_decoder_backend;
mod ffmpeg_encoder_backend;

pub(in crate::services::codec_service) use codec_backend_factory::CodecBackendFactory;
pub(in crate::services::codec_service) use decoder_backend::DecoderBackend;
pub(in crate::services::codec_service) use encoder_backend::EncoderBackend;
pub(in crate::services::codec_service) use ffmpeg_codec_backend_factory::FfmpegCodecBackendFactory;

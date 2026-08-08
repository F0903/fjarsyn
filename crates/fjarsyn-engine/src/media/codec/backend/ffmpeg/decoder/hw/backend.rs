use ffmpeg::{Codec, codec, decoder, frame};
use ffmpeg_next as ffmpeg;

use super::super::{Error, Result};
use crate::media::{
    PixelFormat,
    codec::backend::ffmpeg::{DecoderInfo, HardwareAcceleration},
    frame::Frame,
};

pub(in crate::media::codec::backend::ffmpeg::decoder) struct Backend {
    #[cfg(target_os = "windows")]
    kind: Kind,
}

#[cfg(target_os = "windows")]
enum Kind {
    D3d11va(super::d3d11va::Backend),
}

impl Backend {
    pub(in crate::media::codec::backend::ffmpeg::decoder) fn open_decoder(
        codec: Codec,
        decoder_info: DecoderInfo,
    ) -> Result<(decoder::Video, Option<Self>)> {
        let mut context = codec::context::Context::new_with_codec(codec);
        context.set_flags(codec::Flags::LOW_DELAY);

        let mut hw_backend =
            Self::configure(&codec, &mut context, decoder_info.hardware_acceleration);

        match context.decoder().open_as(codec).and_then(|decoder| decoder.video()) {
            Ok(decoder) => {
                if let Some(backend) = hw_backend.as_ref() {
                    tracing::info!(
                        "Using {} hardware decoding for {}",
                        backend.name(),
                        decoder_info.name
                    );
                }
                Ok((decoder, hw_backend))
            }
            Err(error) => {
                if let Some(backend_name) = hw_backend.as_ref().map(Self::name) {
                    tracing::warn!(
                        "Failed to open {} with {}, falling back to software decode: {}",
                        decoder_info.name,
                        backend_name,
                        error
                    );
                    let _ = hw_backend.take();

                    let mut context = codec::context::Context::new_with_codec(codec);
                    context.set_flags(codec::Flags::LOW_DELAY);
                    let decoder = context
                        .decoder()
                        .open_as(codec)
                        .and_then(|decoder| decoder.video())
                        .map_err(Error::Create)?;

                    return Ok((decoder, None));
                }

                Err(Error::Create(error))
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn configure(
        codec: &Codec,
        context: &mut codec::Context,
        hardware_acceleration: HardwareAcceleration,
    ) -> Option<Self> {
        match hardware_acceleration {
            HardwareAcceleration::D3d11Va => super::d3d11va::Backend::configure(codec, context)
                .map(|backend| Self { kind: Kind::D3d11va(backend) }),
            HardwareAcceleration::None => None,
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn configure(
        _codec: &Codec,
        _context: &mut codec::Context,
        _hardware_acceleration: HardwareAcceleration,
    ) -> Option<Self> {
        None
    }

    pub(in crate::media::codec::backend::ffmpeg::decoder) fn materialize_frame(
        &self,
        decoded_frame: frame::Video,
    ) -> Result<frame::Video> {
        #[cfg(target_os = "windows")]
        match &self.kind {
            Kind::D3d11va(backend) => backend.materialize_frame(decoded_frame),
        }

        #[cfg(not(target_os = "windows"))]
        {
            Ok(decoded_frame)
        }
    }

    pub(in crate::media::codec::backend::ffmpeg::decoder) fn try_decode_frame(
        &self,
        decoded_frame: &frame::Video,
        destination_format: PixelFormat,
    ) -> Result<Option<Frame>> {
        #[cfg(target_os = "windows")]
        match &self.kind {
            Kind::D3d11va(backend) => backend.try_decode_frame(decoded_frame, destination_format),
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (decoded_frame, destination_format);
            Ok(None)
        }
    }

    fn name(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        match &self.kind {
            Kind::D3d11va(backend) => backend.name(),
        }

        #[cfg(not(target_os = "windows"))]
        {
            "software"
        }
    }
}

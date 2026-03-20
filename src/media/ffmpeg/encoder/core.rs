use ffmpeg::{Rational, codec, encoder, format, software::scaling};
use ffmpeg_next as ffmpeg;

use super::{FFmpegEncoder, FFmpegEncoderError, Result};
use crate::{
    media::{ffmpeg::FFmpegTranscodeType, pixel_format::PixelFormat},
    utils::num_utils::align_to_rounded,
};

impl FFmpegEncoder {
    pub(super) fn compute_dst_resolution(&mut self, src_width: i32, src_height: i32) -> (u32, u32) {
        match self.target_resolution {
            crate::media::TargetResolution::Scale(target_size) => (
                align_to_rounded(target_size.width(), 2) as u32,
                align_to_rounded(target_size.height(), 2) as u32,
            ),
            crate::media::TargetResolution::Source => {
                (align_to_rounded(src_width, 2) as u32, align_to_rounded(src_height, 2) as u32)
            }
        }
    }

    pub(super) fn init_encoder(
        &mut self,
        transcoding_type: FFmpegTranscodeType,
        src_width: i32,
        src_height: i32,
        input_format: PixelFormat,
        dst_format: format::Pixel,
    ) -> Result<()> {
        let encoder_info = transcoding_type.get_encoder_info();
        let codec = encoder::find_by_name(encoder_info.name)
            .ok_or(FFmpegEncoderError::Create(ffmpeg::Error::EncoderNotFound))?;
        tracing::info!("Using encoder: {}", codec.name());

        let mut codec_context = codec::Context::new_with_codec(codec)
            .encoder()
            .video()
            .map_err(FFmpegEncoderError::Create)?;

        let (aligned_dst_width, aligned_dst_height) =
            self.compute_dst_resolution(src_width, src_height);

        codec_context.set_width(aligned_dst_width);
        codec_context.set_height(aligned_dst_height);

        let ffmpeg_input_format = input_format.to_ffmpeg_pixel_format();
        codec_context.set_format(dst_format);
        codec_context.set_bit_rate(self.bitrate as usize);

        let time_base = Rational(1, self.target_framerate_hz as i32);
        codec_context.set_time_base(time_base);
        codec_context.set_frame_rate(Some(Rational(self.target_framerate_hz as i32, 1)));

        codec_context.set_gop(Self::GOP_VALUE);
        codec_context.set_max_b_frames(Self::B_FRAMES_VALUE);

        self.init_hw_frames_ctx(
            &mut codec_context,
            aligned_dst_width,
            aligned_dst_height,
            ffmpeg_input_format,
        );

        let mut opts = ffmpeg::Dictionary::new();
        transcoding_type.set_encoder_options(&mut opts);

        tracing::info!(
            "Opening encoder with: width={}, height={}, bitrate={}, time_base={:?}, frame_rate={:?}, gop={}, max_b_frames={}, input_format={:?}",
            aligned_dst_width,
            aligned_dst_height,
            self.bitrate,
            time_base,
            codec_context.frame_rate(),
            Self::GOP_VALUE,
            Self::B_FRAMES_VALUE,
            ffmpeg_input_format
        );

        let encoder = codec_context.open_with(opts).map_err(FFmpegEncoderError::Create)?;
        self.encoder = Some(encoder);

        let scaler = scaling::Context::get(
            ffmpeg_input_format,
            src_width as u32,
            src_height as u32,
            dst_format,
            aligned_dst_width,
            aligned_dst_height,
            Self::SCALING_MODE,
        )
        .map_err(FFmpegEncoderError::Scaler)?;
        self.scaler = Some(scaler);

        self.current_src_width = src_width;
        self.current_src_height = src_height;
        self.current_input_format = Some(input_format);
        self.current_transcoding_type = Some(transcoding_type);

        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub(super) fn init_hw_frames_ctx(
        &self,
        _codec_context: &mut ffmpeg::codec::encoder::video::Video,
        _width: u32,
        _height: u32,
        _sw_format: format::Pixel,
    ) {
    }
}

use ffmpeg::{
    Packet, Rational, codec, encoder,
    software::scaling::{self, Context as Scaler},
    util::format,
};
use ffmpeg_next as ffmpeg;

use crate::media::{
    CodecDeviceLease, PixelFormat,
    codec::{TranscodeType, backend::ffmpeg::HardwareAcceleration},
    frame::Frame,
    video::TargetResolution,
};

pub(super) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("Failed to create encoder: {0}")]
    Create(ffmpeg_next::Error),
    #[error("Failed to encode frame: {0}")]
    Encode(ffmpeg_next::Error),
    #[error("Failed to convert frame: {0}")]
    Conversion(ffmpeg_next::Error),
    #[error("Failed to initialize scaler: {0}")]
    Scaler(ffmpeg_next::Error),
}

fn align_to_even(value: i32) -> i32 {
    (value + 1) & !1
}

pub(crate) struct Encoder {
    pub(super) encoder: Option<encoder::Video>,
    pub(super) scaler: Option<Scaler>,
    pub(super) bitrate: u32,
    pub(super) target_framerate_hz: f32,
    pub(super) target_resolution: TargetResolution,
    pub(super) frame_count: i64,
    pub(super) current_src_width: i32,
    pub(super) current_src_height: i32,
    pub(super) current_input_format: Option<PixelFormat>,
    pub(super) current_transcoding_type: Option<TranscodeType>,
    #[cfg(target_os = "windows")]
    pub(super) hw_device_ctx: Option<*mut ffmpeg_next::ffi::AVBufferRef>,
}

impl Encoder {
    pub(super) const GOP_VALUE: u32 = 120;
    pub(super) const B_FRAMES_VALUE: usize = 0;
    pub(super) const SCALING_MODE: scaling::Flags = scaling::Flags::BILINEAR;

    pub(crate) fn new(
        bitrate: u32,
        target_framerate_hz: f32,
        target_resolution: TargetResolution,
        device: Option<CodecDeviceLease>,
        transcoding_type: TranscodeType,
    ) -> Result<Self> {
        ffmpeg::init().map_err(Error::Create)?;

        #[cfg(debug_assertions)]
        ffmpeg::log::set_level(ffmpeg::log::Level::Debug);

        #[cfg(target_os = "windows")]
        let hw_device_ctx = match transcoding_type.encoder_info().hardware_acceleration {
            HardwareAcceleration::D3d11Va => device.as_ref().and_then(Self::init_hw_device_ctx),
            HardwareAcceleration::None => None,
        };

        Ok(Self {
            encoder: None,
            scaler: None,
            bitrate,
            target_framerate_hz,
            target_resolution,
            frame_count: 0,
            current_src_width: 0,
            current_src_height: 0,
            current_input_format: None,
            current_transcoding_type: None,
            #[cfg(target_os = "windows")]
            hw_device_ctx,
        })
    }

    pub(crate) fn encode(
        &mut self,
        frame: &Frame,
        transcoding_type: TranscodeType,
        force_keyframe: bool,
    ) -> Result<Vec<Vec<u8>>> {
        let width = frame.size.width;
        let height = frame.size.height;
        let dst_format = transcoding_type.encoder_info().scaler_format;

        if self.encoder.is_none()
            || self.current_src_width != width
            || self.current_src_height != height
            || self.current_input_format != Some(frame.format)
            || self.current_transcoding_type != Some(transcoding_type)
        {
            self.init_encoder(transcoding_type, width, height, frame.format, dst_format)?;
        }

        let (dst_w, dst_h) = self.compute_dst_resolution(width, height);

        #[cfg(target_os = "windows")]
        if transcoding_type.encoder_info().hardware_acceleration == HardwareAcceleration::D3d11Va
            && let Some(texture) = frame.d3d11_texture()
            && self.hw_device_ctx.is_some()
        {
            self.encode_d3d11(texture, width, height, dst_w, dst_h, force_keyframe)?;
            return self.collect_nal_units();
        }

        self.encode_software(frame, dst_w, dst_h, dst_format, force_keyframe)?;
        self.collect_nal_units()
    }

    pub(super) fn compute_dst_resolution(&mut self, src_width: i32, src_height: i32) -> (u32, u32) {
        match self.target_resolution {
            TargetResolution::Scale(target_size) => {
                (align_to_even(target_size.width) as u32, align_to_even(target_size.height) as u32)
            }
            TargetResolution::Source => {
                (align_to_even(src_width) as u32, align_to_even(src_height) as u32)
            }
        }
    }

    pub(super) fn init_encoder(
        &mut self,
        transcoding_type: TranscodeType,
        src_width: i32,
        src_height: i32,
        input_format: PixelFormat,
        dst_format: format::Pixel,
    ) -> Result<()> {
        let encoder_info = transcoding_type.encoder_info();
        let codec = encoder::find_by_name(encoder_info.name)
            .ok_or(Error::Create(ffmpeg::Error::EncoderNotFound))?;
        tracing::info!("Using encoder: {}", codec.name());

        let mut codec_context =
            codec::Context::new_with_codec(codec).encoder().video().map_err(Error::Create)?;

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

        let encoder = codec_context.open_with(opts).map_err(Error::Create)?;
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
        .map_err(Error::Scaler)?;
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

    pub(super) fn collect_nal_units(&mut self) -> Result<Vec<Vec<u8>>> {
        let encoder = self.encoder.as_mut().unwrap();
        let mut nal_units = Vec::new();
        let mut packet = Packet::empty();
        loop {
            match encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    if let Some(data) = packet.data() {
                        nal_units.push(data.as_ref().to_vec());
                    }
                }
                Err(error) if receive_is_drained(&error) => break,
                Err(error) => return Err(Error::Encode(error)),
            }
        }
        Ok(nal_units)
    }
}

fn receive_is_drained(error: &ffmpeg::Error) -> bool {
    matches!(
        error,
        ffmpeg::Error::Other { errno } if *errno == ffmpeg::error::EAGAIN
    ) || matches!(error, ffmpeg::Error::Eof)
}

impl std::fmt::Debug for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Encoder")
            .field("bitrate", &self.bitrate)
            .field("target_framerate_hz", &self.target_framerate_hz)
            .field("target_resolution", &self.target_resolution)
            .field("frame_count", &self.frame_count)
            .finish()
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if let Some(mut ctx) = self.hw_device_ctx.take() {
            unsafe {
                ffmpeg_next::ffi::av_buffer_unref(&mut ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use ffmpeg_next as ffmpeg;

    use super::{Encoder, receive_is_drained};
    use crate::media::{
        Dimensions, PixelFormat,
        codec::TranscodeType,
        frame::{Frame, FrameData},
        video::TargetResolution,
    };

    const SPS_NAL_TYPE: u8 = 7;
    const PPS_NAL_TYPE: u8 = 8;
    const IDR_NAL_TYPE: u8 = 5;

    #[test]
    fn only_eagain_and_eof_finish_packet_draining() {
        assert!(receive_is_drained(&ffmpeg::Error::Other { errno: ffmpeg::error::EAGAIN }));
        assert!(receive_is_drained(&ffmpeg::Error::Eof));
        assert!(!receive_is_drained(&ffmpeg::Error::InvalidData));
    }

    #[test]
    fn later_forced_software_keyframe_repeats_sps_pps_and_idr() {
        let mut encoder = Encoder::new(
            1_000_000,
            30.0,
            TargetResolution::Source,
            None,
            TranscodeType::H264Software,
        )
        .unwrap();
        let frame = solid_software_frame(16, 16);

        let initial = encoder.encode(&frame, TranscodeType::H264Software, false).unwrap();
        assert_bootstrap_access_unit(&initial);

        for _ in 0..2 {
            let dependent = encoder.encode(&frame, TranscodeType::H264Software, false).unwrap();
            let nal_types = access_unit_nal_types(&dependent);
            assert!(!nal_types.contains(&SPS_NAL_TYPE));
            assert!(!nal_types.contains(&PPS_NAL_TYPE));
            assert!(!nal_types.contains(&IDR_NAL_TYPE));
        }

        let forced = encoder.encode(&frame, TranscodeType::H264Software, true).unwrap();
        assert_bootstrap_access_unit(&forced);
    }

    fn solid_software_frame(width: i32, height: i32) -> Frame {
        let pixel_count = usize::try_from(width * height).unwrap();
        let pixels = [20, 40, 60, 255].repeat(pixel_count);
        Frame {
            data: FrameData::Software(Bytes::from(pixels)),
            format: PixelFormat::RGBA8,
            size: Dimensions::new(width, height),
            duration: None,
        }
    }

    fn assert_bootstrap_access_unit(packets: &[Vec<u8>]) {
        let packet_nal_types =
            packets.iter().map(|packet| annex_b_nal_types(packet)).collect::<Vec<_>>();
        assert!(
            packet_nal_types.iter().any(|nal_types| {
                nal_types.contains(&SPS_NAL_TYPE)
                    && nal_types.contains(&PPS_NAL_TYPE)
                    && nal_types.contains(&IDR_NAL_TYPE)
            }),
            "no self-contained SPS/PPS/IDR access unit in {packet_nal_types:?}"
        );
    }

    fn access_unit_nal_types(packets: &[Vec<u8>]) -> Vec<u8> {
        packets.iter().flat_map(|packet| annex_b_nal_types(packet)).collect()
    }

    fn annex_b_nal_types(data: &[u8]) -> Vec<u8> {
        let mut nal_types = Vec::new();
        let mut offset = 0;
        while let Some((start, start_code_len)) = find_start_code(data, offset) {
            let nal_offset = start + start_code_len;
            if let Some(header) = data.get(nal_offset) {
                nal_types.push(header & 0x1f);
            }
            offset = nal_offset.saturating_add(1);
        }
        nal_types
    }

    fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
        (from..data.len()).find_map(|offset| {
            if data[offset..].starts_with(&[0, 0, 0, 1]) {
                Some((offset, 4))
            } else if data[offset..].starts_with(&[0, 0, 1]) {
                Some((offset, 3))
            } else {
                None
            }
        })
    }
}

use ffmpeg::{
    encoder,
    software::scaling::{self, Context as Scaler},
};
use ffmpeg_next as ffmpeg;

mod core;
mod packets;
mod software;

#[cfg(target_os = "windows")]
mod windows;

use crate::media::{
    TargetResolution,
    ffmpeg::ffmpeg_transcode_type::{FFmpegTranscodeType, HWAccelType},
    frame::Frame,
    pixel_format::PixelFormat,
};

type Result<T> = std::result::Result<T, FFmpegEncoderError>;

#[derive(Debug, thiserror::Error)]
pub enum FFmpegEncoderError {
    #[error("Failed to create encoder: {0}")]
    Create(ffmpeg::Error),
    #[error("Failed to encode frame: {0}")]
    Encode(ffmpeg::Error),
    #[error("Failed to convert frame: {0}")]
    Conversion(ffmpeg::Error),
    #[error("Failed to initialize scaler: {0}")]
    Scaler(ffmpeg::Error),
}

pub struct FFmpegEncoder {
    input_format: PixelFormat,
    encoder: Option<encoder::Video>,
    scaler: Option<Scaler>,
    bitrate: u32,
    target_framerate_hz: f32,
    target_resolution: TargetResolution,
    frame_count: i64,
    current_src_width: i32,
    current_src_height: i32,
    current_input_format: Option<PixelFormat>,
    current_transcoding_type: Option<FFmpegTranscodeType>,
    #[cfg(target_os = "windows")]
    hw_device_ctx: Option<*mut ffmpeg_next::ffi::AVBufferRef>,
}

impl FFmpegEncoder {
    pub(super) const GOP_VALUE: u32 = 120;
    pub(super) const B_FRAMES_VALUE: usize = 0;
    pub(super) const SCALING_MODE: scaling::Flags = scaling::Flags::BILINEAR;

    pub fn new(
        bitrate: u32,
        target_framerate_hz: f32,
        target_resolution: TargetResolution,
        input_format: PixelFormat,
        device_handle: Option<*mut std::ffi::c_void>,
        transcoding_type: FFmpegTranscodeType,
    ) -> Result<Self> {
        ffmpeg::init().map_err(FFmpegEncoderError::Create)?;

        #[cfg(debug_assertions)]
        ffmpeg::log::set_level(ffmpeg::log::Level::Debug);

        #[cfg(target_os = "windows")]
        let hw_device_ctx = match transcoding_type.get_encoder_info().hw_accel {
            HWAccelType::D3D11VA => device_handle.and_then(Self::init_hw_device_ctx),
            HWAccelType::None => None,
        };

        Ok(Self {
            input_format,
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

    pub fn encode(
        &mut self,
        frame: &Frame,
        transcoding_type: FFmpegTranscodeType,
        width: i32,
        height: i32,
    ) -> Result<Vec<Vec<u8>>> {
        let dst_format = transcoding_type.get_encoder_info().scaler_format;

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
        if transcoding_type.get_encoder_info().hw_accel == HWAccelType::D3D11VA
            && let Some(texture) = frame.d3d11_texture()
            && self.hw_device_ctx.is_some()
        {
            self.encode_d3d11(texture, width, height, dst_w, dst_h)?;
            return self.collect_nal_units();
        }

        self.encode_software(frame, width, height, dst_w, dst_h, dst_format)?;
        self.collect_nal_units()
    }
}

impl std::fmt::Debug for FFmpegEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FFmpegEncoder")
            .field("bitrate", &self.bitrate)
            .field("target_framerate_hz", &self.target_framerate_hz)
            .field("target_resolution", &self.target_resolution)
            .field("frame_count", &self.frame_count)
            .finish()
    }
}

unsafe impl Send for FFmpegEncoder {}

impl Drop for FFmpegEncoder {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if let Some(mut ctx) = self.hw_device_ctx.take() {
            unsafe {
                ffmpeg_next::ffi::av_buffer_unref(&mut ctx);
            }
        }
    }
}

use ffmpeg::{
    Packet, Rational, codec, encoder, format, frame,
    software::scaling::{self, Context as Scaler},
    sys,
};
use ffmpeg_next as ffmpeg;

use crate::{media::ffmpeg::FFmpegTranscodeType, utils::pixel_format::PixelFormat};

type Result<T> = std::result::Result<T, FFmpegEncoderError>;

#[derive(Debug, thiserror::Error)]
pub enum FFmpegEncoderError {
    #[error("Failed to create encoder: {0}")]
    CreateEncoderError(ffmpeg::Error),
    #[error("Failed to encode frame: {0}")]
    EncodeError(ffmpeg::Error),
    #[error("Failed to convert frame: {0}")]
    ConversionError(ffmpeg::Error),
    #[error("Failed to initialize scaler: {0}")]
    ScalerError(ffmpeg::Error),
    #[error("Invalid parameters")]
    InvalidParameters,
    #[error("Failed to find HW device")]
    HWDeviceNotFound,
    #[error("Failed to create HW device: {0}")]
    HWDeviceError(ffmpeg::Error),
    #[error("Failed to create HW frames context: {0}")]
    HWFramesError(ffmpeg::Error),
    #[error("Hardware upload failed: {0}")]
    HWUploadError(ffmpeg::Error),
}

pub struct FFmpegEncoder {
    input_format: PixelFormat,
    encoder: Option<encoder::Video>,
    scaler: Option<Scaler>,
    bitrate: u32,
    target_framerate_hz: f32,
    frame_count: i64,
    hw_device_ctx: Option<*mut sys::AVBufferRef>,
    hw_frames_ctx: Option<*mut sys::AVBufferRef>,
}

impl Drop for FFmpegEncoder {
    fn drop(&mut self) {
        unsafe {
            if let Some(mut ctx) = self.hw_frames_ctx {
                sys::av_buffer_unref(&mut ctx);
            }
            if let Some(mut ctx) = self.hw_device_ctx {
                sys::av_buffer_unref(&mut ctx);
            }
        }
    }
}

impl FFmpegEncoder {
    const DST_FORMAT: format::Pixel = format::Pixel::NV12;
    const GOP_VALUE: u32 = 120;
    const B_FRAMES_VALUE: usize = 0;
    const SCALING_MODE: scaling::Flags = scaling::Flags::BILINEAR;

    pub fn new(bitrate: u32, target_framerate_hz: f32, input_format: PixelFormat) -> Result<Self> {
        ffmpeg::init().map_err(FFmpegEncoderError::CreateEncoderError)?;

        Ok(Self {
            input_format,
            encoder: None,
            scaler: None,
            bitrate,
            target_framerate_hz,
            frame_count: 0,
            hw_device_ctx: None,
            hw_frames_ctx: None,
        })
    }

    fn init_encoder(
        &mut self,
        transcoding_type: FFmpegTranscodeType,
        width: i32,
        height: i32,
    ) -> Result<()> {
        let codec = encoder::find_by_name(transcoding_type.to_encoder_name())
            .or_else(|| {
                tracing::info!("Specified encoder not found, using fallback.");
                encoder::find(codec::Id::H264)
            })
            .ok_or(FFmpegEncoderError::CreateEncoderError(ffmpeg::Error::EncoderNotFound))?;

        tracing::info!("Using encoder: {}", codec.name());

        let mut context = codec::Context::new_with_codec(codec)
            .encoder()
            .video()
            .map_err(FFmpegEncoderError::CreateEncoderError)?;

        // Align resolution to even numbers
        let aligned_width = width & !1;
        let aligned_height = height & !1;

        context.set_width(aligned_width as u32);
        context.set_height(aligned_height as u32);
        context.set_format(transcoding_type.get_input_format());
        context.set_bit_rate(self.bitrate as usize);

        let time_base = Rational(1, self.target_framerate_hz as i32);
        context.set_time_base(time_base);
        context.set_frame_rate(Some(Rational(self.target_framerate_hz as i32, 1)));

        context.set_gop(Self::GOP_VALUE);
        context.set_max_b_frames(Self::B_FRAMES_VALUE);

        // Hardware Context Initialization
        if let Some(device_type) = transcoding_type.hw_accel_name() {
            unsafe {
                let device_type = std::ffi::CString::new(device_type).unwrap();
                let type_enum = sys::av_hwdevice_find_type_by_name(device_type.as_ptr());
                if type_enum == sys::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
                    return Err(FFmpegEncoderError::HWDeviceNotFound);
                }

                let mut device_ctx_ref: *mut sys::AVBufferRef = std::ptr::null_mut();
                let ret = sys::av_hwdevice_ctx_create(
                    &mut device_ctx_ref,
                    type_enum,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    0,
                );
                if ret < 0 {
                    return Err(FFmpegEncoderError::HWDeviceError(ffmpeg::Error::from(ret)));
                }
                self.hw_device_ctx = Some(device_ctx_ref);

                let frames_ctx_ref = sys::av_hwframe_ctx_alloc(device_ctx_ref);
                if frames_ctx_ref.is_null() {
                    return Err(FFmpegEncoderError::HWFramesError(ffmpeg::Error::Unknown));
                }
                self.hw_frames_ctx = Some(frames_ctx_ref);

                let frames_ctx = (*frames_ctx_ref).data as *mut sys::AVHWFramesContext;
                (*frames_ctx).format = transcoding_type.get_input_format().into();
                (*frames_ctx).sw_format = Self::DST_FORMAT.into();
                (*frames_ctx).width = aligned_width;
                (*frames_ctx).height = aligned_height;
                (*frames_ctx).initial_pool_size = 20;

                let ret = sys::av_hwframe_ctx_init(frames_ctx_ref);
                if ret < 0 {
                    return Err(FFmpegEncoderError::HWFramesError(ffmpeg::Error::from(ret)));
                }

                // Attach to encoder context
                // We must use a new reference because the encoder takes ownership of one ref
                let encoder_frames_ref = sys::av_buffer_ref(frames_ctx_ref);
                if encoder_frames_ref.is_null() {
                    return Err(FFmpegEncoderError::HWFramesError(ffmpeg::Error::Unknown));
                }
                (*context.as_mut_ptr()).hw_frames_ctx = encoder_frames_ref;
                (*context.as_mut_ptr()).hw_device_ctx = sys::av_buffer_ref(device_ctx_ref);
            }
        }

        let mut opts = ffmpeg::Dictionary::new();
        transcoding_type.set_encoder_options(&mut opts);

        let encoder = context.open_with(opts).map_err(FFmpegEncoderError::CreateEncoderError)?;
        self.encoder = Some(encoder);

        let scaler = scaling::Context::get(
            self.input_format.to_ffmpeg_pixel_format(),
            width as u32,
            height as u32,
            Self::DST_FORMAT,
            aligned_width as u32,
            aligned_height as u32,
            Self::SCALING_MODE,
        )
        .map_err(FFmpegEncoderError::ScalerError)?;
        self.scaler = Some(scaler);

        Ok(())
    }

    /// Encodes a raw RGBA8 bitmap into a list of H.264 NAL units (as packets).
    pub fn encode(
        &mut self,
        bitmap: &[u8],
        transcoding_type: FFmpegTranscodeType,
        width: i32,
        height: i32,
    ) -> Result<Vec<Vec<u8>>> {
        if self.encoder.is_none() {
            self.init_encoder(transcoding_type, width, height)?;
        }

        // Align resolution to even numbers
        let aligned_width = width & !1;
        let aligned_height = height & !1;

        if let Some(enc) = &self.encoder {
            if enc.width() != aligned_width as u32 || enc.height() != aligned_height as u32 {
                // Re-init
                self.init_encoder(transcoding_type, width, height)?;
            }
        }

        let encoder = self.encoder.as_mut().unwrap();
        let scaler = self.scaler.as_mut().unwrap();

        let mut input_frame = frame::Video::new(
            self.input_format.to_ffmpeg_pixel_format(),
            width as u32,
            height as u32,
        );
        input_frame.data_mut(0).copy_from_slice(bitmap);

        let mut dst_frame =
            frame::Video::new(Self::DST_FORMAT, aligned_width as u32, aligned_height as u32);
        scaler.run(&input_frame, &mut dst_frame).map_err(FFmpegEncoderError::ConversionError)?;

        dst_frame.set_pts(Some(self.frame_count));
        self.frame_count += 1;

        if let Some(frames_ctx_ref) = self.hw_frames_ctx {
            unsafe {
                let mut hw_frame = frame::Video::empty();
                // Access raw pointer for get_buffer
                let ret = sys::av_hwframe_get_buffer(frames_ctx_ref, hw_frame.as_mut_ptr(), 0);
                if ret < 0 {
                    return Err(FFmpegEncoderError::HWUploadError(ffmpeg::Error::from(ret)));
                }

                // Transfer data
                let ret =
                    sys::av_hwframe_transfer_data(hw_frame.as_mut_ptr(), dst_frame.as_ptr(), 0);
                if ret < 0 {
                    return Err(FFmpegEncoderError::HWUploadError(ffmpeg::Error::from(ret)));
                }

                // Copy PTS
                (*hw_frame.as_mut_ptr()).pts = dst_frame.pts().unwrap_or(0);

                encoder.send_frame(&hw_frame).map_err(FFmpegEncoderError::EncodeError)?;
            }
        } else {
            encoder.send_frame(&dst_frame).map_err(FFmpegEncoderError::EncodeError)?;
        }

        let mut nal_units = Vec::new();
        let mut packet = Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            // We treat the packet data as a "NAL unit" blob.
            if let Some(data) = packet.data() {
                nal_units.push(data.to_vec());
            }
        }

        Ok(nal_units)
    }
}

impl std::fmt::Debug for FFmpegEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FFmpegEncoder")
            .field("bitrate", &self.bitrate)
            .field("target_framerate_hz", &self.target_framerate_hz)
            .field("frame_count", &self.frame_count)
            .finish()
    }
}

unsafe impl Send for FFmpegEncoder {}

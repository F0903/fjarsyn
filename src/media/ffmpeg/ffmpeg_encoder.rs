use ffmpeg::{
    Packet, Rational, codec, encoder, format,
    software::scaling::{self, Context as Scaler},
};
use ffmpeg_next as ffmpeg;

use crate::{
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
    utils::{
        frame::{Frame, FrameData},
        num_utils::align_to_rounded,
        pixel_format::PixelFormat,
    },
};

#[repr(C)]
pub struct AVD3D11VADeviceContext {
    pub device: *mut windows::Win32::Graphics::Direct3D11::ID3D11Device,
    pub device_context: *mut windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    pub video_device: *mut std::ffi::c_void,
    pub video_context: *mut std::ffi::c_void,
    pub lock: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void)>,
    pub unlock: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void)>,
    pub lock_ctx: *mut std::ffi::c_void,
}

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
    hw_device_ctx: Option<*mut ffmpeg_next::ffi::AVBufferRef>,
}

impl FFmpegEncoder {
    const GOP_VALUE: u32 = 120;
    const B_FRAMES_VALUE: usize = 0;
    const SCALING_MODE: scaling::Flags = scaling::Flags::BILINEAR;

    pub fn new(
        bitrate: u32,
        target_framerate_hz: f32,
        target_resolution: TargetResolution,
        input_format: PixelFormat,
        device_handle: Option<*mut std::ffi::c_void>,
    ) -> Result<Self> {
        ffmpeg::init().map_err(FFmpegEncoderError::Create)?;

        #[cfg(debug_assertions)]
        ffmpeg::log::set_level(ffmpeg::log::Level::Debug);

        let mut hw_device_ctx = None;
        if let Some(handle) = device_handle {
            unsafe {
                let ctx = ffmpeg_next::ffi::av_hwdevice_ctx_alloc(
                    ffmpeg_next::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
                );
                if !ctx.is_null() {
                    let hw_ctx = (*ctx).data as *mut ffmpeg_next::ffi::AVHWDeviceContext;
                    let d3d11_ctx = (*hw_ctx).hwctx as *mut AVD3D11VADeviceContext;

                    (*d3d11_ctx).device = handle as *mut _;

                    let ret = ffmpeg_next::ffi::av_hwdevice_ctx_init(ctx);
                    if ret < 0 {
                        tracing::error!("Failed to initialize hardware device context: {}", ret);
                        let mut p = ctx;
                        ffmpeg_next::ffi::av_buffer_unref(&mut p);
                    } else {
                        tracing::info!("Successfully initialized D3D11VA hardware device context");
                        hw_device_ctx = Some(ctx);
                    }
                } else {
                    tracing::error!("Failed to allocate hardware device context");
                }
            }
        }

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
            hw_device_ctx,
        })
    }

    fn compute_dst_resolution(&mut self, src_width: i32, src_height: i32) -> (u32, u32) {
        // Align resolution to 2 (requirement for some formats)
        match self.target_resolution {
            TargetResolution::Scale(target_size) => (
                align_to_rounded(target_size.width(), 2) as u32,
                align_to_rounded(target_size.height(), 2) as u32,
            ),
            TargetResolution::Source => {
                (align_to_rounded(src_width, 2) as u32, align_to_rounded(src_height, 2) as u32)
            }
        }
    }

    fn init_encoder(
        &mut self,
        transcoding_type: FFmpegTranscodeType,
        src_width: i32,
        src_height: i32,
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

        let (aligned_src_width, aligned_src_height) =
            (align_to_rounded(src_width, 2), align_to_rounded(src_height, 2));
        let (aligned_dst_width, aligned_dst_height) =
            self.compute_dst_resolution(src_width, src_height);

        codec_context.set_width(aligned_dst_width);
        codec_context.set_height(aligned_dst_height);
        codec_context.set_format(encoder_info.input_format);
        codec_context.set_bit_rate(self.bitrate as usize);

        let time_base = Rational(1, self.target_framerate_hz as i32);
        codec_context.set_time_base(time_base);
        codec_context.set_frame_rate(Some(Rational(self.target_framerate_hz as i32, 1)));

        codec_context.set_gop(Self::GOP_VALUE);
        codec_context.set_max_b_frames(Self::B_FRAMES_VALUE);

        // Hardware context setup
        if let Some(hw_device_ctx) = self.hw_device_ctx {
            unsafe {
                let mut frames_ctx_ref = ffmpeg_next::ffi::av_hwframe_ctx_alloc(hw_device_ctx);
                if !frames_ctx_ref.is_null() {
                    let frames_ctx =
                        (*frames_ctx_ref).data as *mut ffmpeg_next::ffi::AVHWFramesContext;
                    (*frames_ctx).format = ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_D3D11;
                    (*frames_ctx).sw_format = ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_NV12;
                    (*frames_ctx).width = aligned_dst_width as i32;
                    (*frames_ctx).height = aligned_dst_height as i32;
                    (*frames_ctx).initial_pool_size = 0;

                    let ret = ffmpeg_next::ffi::av_hwframe_ctx_init(frames_ctx_ref);
                    if ret >= 0 {
                        tracing::info!("Initialized D3D11 hardware frames context");
                        (*codec_context.as_mut_ptr()).hw_frames_ctx =
                            ffmpeg_next::ffi::av_buffer_ref(frames_ctx_ref);
                        // Force the encoder to use the hardware format
                        codec_context.set_format(format::Pixel::D3D11);
                    } else {
                        tracing::error!("Failed to initialize hardware frames context: {}", ret);
                    }
                    ffmpeg_next::ffi::av_buffer_unref(&mut frames_ctx_ref);
                }
            }
        }

        let mut opts = ffmpeg::Dictionary::new();
        transcoding_type.set_encoder_options(&mut opts);

        tracing::info!(
            "Opening encoder with: width={}, height={}, bitrate={}, time_base={:?}, frame_rate={:?}, gop={}, max_b_frames={}, format={:?}",
            aligned_src_width,
            aligned_src_height,
            self.bitrate,
            time_base,
            codec_context.frame_rate(),
            Self::GOP_VALUE,
            Self::B_FRAMES_VALUE,
            encoder_info.input_format
        );

        let encoder = codec_context.open_with(opts).map_err(FFmpegEncoderError::Create)?;
        self.encoder = Some(encoder);

        let scaler = scaling::Context::get(
            self.input_format.to_ffmpeg_pixel_format(),
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

        Ok(())
    }

    /// Encodes a frame into a list of NAL units (as packets).
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
        {
            self.init_encoder(transcoding_type, width, height, dst_format)?;
        }

        let (dst_w, dst_h) = self.compute_dst_resolution(width, height);
        let encoder = self.encoder.as_mut().unwrap();
        let mut input_frame = ffmpeg::frame::Video::empty();

        match &frame.data {
            #[cfg(target_os = "windows")]
            FrameData::D3D11 { texture, .. } if self.hw_device_ctx.is_some() => {
                input_frame.set_format(format::Pixel::D3D11);
                input_frame.set_width(width as u32);
                input_frame.set_height(height as u32);
                unsafe {
                    let ptr = input_frame.as_mut_ptr();
                    (*ptr).data[0] = windows_core::Interface::as_raw(texture) as *mut u8;
                }

                input_frame.set_pts(Some(self.frame_count));
                self.frame_count += 1;

                encoder.send_frame(&input_frame).map_err(FFmpegEncoderError::Encode)?;

                // CLEANUP: Nullify the pointers so `input_frame`'s Drop doesn't free our borrowed handle.
                unsafe {
                    let ptr = input_frame.as_mut_ptr();
                    (*ptr).data[0] = std::ptr::null_mut();
                }
            }
            _ => {
                // Software path (or D3D11 fallback if no hw_device_ctx)
                let pixels = match &frame.data {
                    FrameData::Software(buf) => buf,
                    #[cfg(target_os = "windows")]
                    FrameData::D3D11 { mapped_buffer, .. } => {
                        mapped_buffer.as_ref().ok_or_else(|| {
                            FFmpegEncoderError::Conversion(ffmpeg::Error::InvalidData)
                        })?
                    }
                };

                input_frame.set_format(self.input_format.to_ffmpeg_pixel_format());
                input_frame.set_width(width as u32);
                input_frame.set_height(height as u32);

                unsafe {
                    let ptr = input_frame.as_mut_ptr();
                    let stride = width * self.input_format.bytes_per_pixel() as i32;

                    // Set data pointers
                    (*ptr).data[0] = pixels.as_ptr() as *mut u8;
                    (*ptr).linesize[0] = stride;
                    (*ptr).extended_data = (*ptr).data.as_mut_ptr();
                }

                let mut dst_frame = ffmpeg::frame::Video::new(dst_format, dst_w, dst_h);
                let scaler = self.scaler.as_mut().unwrap();

                let scale_result = scaler.run(&input_frame, &mut dst_frame);

                // CLEANUP: Nullify the pointers so `input_frame`'s Drop doesn't free our borrowed slice.
                unsafe {
                    let ptr = input_frame.as_mut_ptr();
                    (*ptr).data[0] = std::ptr::null_mut();
                    (*ptr).linesize[0] = 0;
                    (*ptr).extended_data = std::ptr::null_mut();
                }

                if let Err(e) = scale_result {
                    return Err(FFmpegEncoderError::Conversion(e));
                }

                dst_frame.set_pts(Some(self.frame_count));
                self.frame_count += 1;

                encoder.send_frame(&dst_frame).map_err(FFmpegEncoderError::Encode)?;
            }
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
            .field("target_resolution", &self.target_resolution)
            .field("frame_count", &self.frame_count)
            .finish()
    }
}

unsafe impl Send for FFmpegEncoder {}

impl Drop for FFmpegEncoder {
    fn drop(&mut self) {
        if let Some(mut ctx) = self.hw_device_ctx.take() {
            unsafe {
                ffmpeg_next::ffi::av_buffer_unref(&mut ctx);
            }
        }
    }
}

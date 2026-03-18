use ffmpeg::{
    Packet, Rational, codec, encoder, format,
    software::scaling::{self, Context as Scaler},
};
use ffmpeg_next as ffmpeg;
use windows_core::Interface;

#[cfg(target_os = "windows")]
use crate::{
    media::{
        TargetResolution,
        ffmpeg::{FFmpegTranscodeType, HWAccelType},
        frame::Frame,
        pixel_format::PixelFormat,
    },
    utils::num_utils::align_to_rounded,
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
    current_input_format: Option<PixelFormat>,
    current_transcoding_type: Option<FFmpegTranscodeType>,
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
        transcoding_type: FFmpegTranscodeType,
    ) -> Result<Self> {
        ffmpeg::init().map_err(FFmpegEncoderError::Create)?;

        #[cfg(debug_assertions)]
        ffmpeg::log::set_level(ffmpeg::log::Level::Debug);

        let hw_accel = transcoding_type.get_encoder_info().hw_accel;
        let hw_device_ctx = match hw_accel {
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
            hw_device_ctx,
        })
    }

    fn init_hw_device_ctx(
        handle: *mut std::ffi::c_void,
    ) -> Option<*mut ffmpeg_next::ffi::AVBufferRef> {
        unsafe {
            let ctx = ffmpeg_next::ffi::av_hwdevice_ctx_alloc(
                ffmpeg_next::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            );
            if !ctx.is_null() {
                let hw_ctx = (*ctx).data as *mut ffmpeg_next::ffi::AVHWDeviceContext;
                let d3d11_ctx = (*hw_ctx).hwctx as *mut AVD3D11VADeviceContext;

                // Use ManuallyDrop since FFmpeg will take ownership of the handle
                let device: std::mem::ManuallyDrop<
                    windows::Win32::Graphics::Direct3D11::ID3D11Device,
                > = std::mem::ManuallyDrop::new(std::mem::transmute_copy(&handle));
                let device_context = device.GetImmediateContext().ok();

                (*d3d11_ctx).device = handle as *mut _;
                if let Some(context) = device_context {
                    // Transfer ownership to FFmpeg
                    (*d3d11_ctx).device_context =
                        std::mem::ManuallyDrop::new(context).as_raw() as *mut _;
                }

                let ret = ffmpeg_next::ffi::av_hwdevice_ctx_init(ctx);
                if ret < 0 {
                    tracing::error!("Failed to initialize hardware device context: {}", ret);
                    let mut p = ctx;
                    ffmpeg_next::ffi::av_buffer_unref(&mut p);
                    None
                } else {
                    tracing::info!("Successfully initialized D3D11VA hardware device context");
                    Some(ctx)
                }
            } else {
                tracing::error!("Failed to allocate hardware device context");
                None
            }
        }
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

        // Hardware context setup
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

    fn init_hw_frames_ctx(
        &self,
        codec_context: &mut ffmpeg::codec::encoder::video::Video,
        width: u32,
        height: u32,
        sw_format: format::Pixel,
    ) {
        let Some(hw_device_ctx) = self.hw_device_ctx else {
            return;
        };

        unsafe {
            let mut frames_ctx_ref = ffmpeg_next::ffi::av_hwframe_ctx_alloc(hw_device_ctx);
            if !frames_ctx_ref.is_null() {
                let frames_ctx = (*frames_ctx_ref).data as *mut ffmpeg_next::ffi::AVHWFramesContext;
                (*frames_ctx).format = ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_D3D11;
                (*frames_ctx).sw_format = sw_format.into();
                (*frames_ctx).width = width as i32;
                (*frames_ctx).height = height as i32;
                (*frames_ctx).initial_pool_size = 0;

                let ret = ffmpeg_next::ffi::av_hwframe_ctx_init(frames_ctx_ref);
                if ret >= 0 {
                    tracing::info!("Initialized D3D11 hardware frames context ({:?})", sw_format);
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

    #[cfg(target_os = "windows")]
    fn encode_d3d11(
        &mut self,
        texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        width: i32,
        height: i32,
        dst_w: u32,
        dst_h: u32,
    ) -> Result<()> {
        let encoder = self.encoder.as_mut().unwrap();
        let mut input_frame = ffmpeg::frame::Video::empty();
        let hw_device_ctx = self.hw_device_ctx.unwrap();

        unsafe {
            let hw_ctx = (*hw_device_ctx).data as *mut ffmpeg_next::ffi::AVHWDeviceContext;
            let d3d11_ctx = (*hw_ctx).hwctx as *mut AVD3D11VADeviceContext;
            let device_context_ptr = (*d3d11_ctx).device_context;

            if device_context_ptr.is_null() {
                tracing::error!("D3D11DeviceContext is null in AVHWDeviceContext");
                return Err(FFmpegEncoderError::Encode(ffmpeg::Error::InvalidData));
            }

            let device_context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext =
                std::mem::transmute(&device_context_ptr);

            let codec_context = encoder.as_mut_ptr();
            let hw_frames_ctx = (*codec_context).hw_frames_ctx;

            if hw_frames_ctx.is_null() {
                tracing::error!("hw_frames_ctx is null on encoder context");
                return Err(FFmpegEncoderError::Encode(ffmpeg::Error::InvalidData));
            }

            // Request a new hardware frame from FFmpeg's pool
            let ret =
                ffmpeg_next::ffi::av_hwframe_get_buffer(hw_frames_ctx, input_frame.as_mut_ptr(), 0);
            if ret < 0 {
                tracing::error!("av_hwframe_get_buffer failed: {}", ret);
                return Err(FFmpegEncoderError::Encode(ffmpeg::Error::InvalidData));
            }

            // Extract FFmpeg's destination texture
            let dst_texture_ptr = (*input_frame.as_mut_ptr()).data[0] as *mut std::ffi::c_void;
            let dst_subresource = (*input_frame.as_mut_ptr()).data[1] as usize as u32;

            if dst_texture_ptr.is_null() {
                tracing::error!("Allocated hardware frame has null texture pointer");
                return Err(FFmpegEncoderError::Encode(ffmpeg::Error::InvalidData));
            }

            let dst_texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D =
                std::mem::transmute(&dst_texture_ptr);

            // We might need an exclusive lock if the context is shared?
            if let Some(lock) = (*d3d11_ctx).lock {
                lock((*d3d11_ctx).lock_ctx);
            }

            let mut src_desc = std::mem::zeroed();
            texture.GetDesc(&mut src_desc);
            let mut dst_desc = std::mem::zeroed();
            dst_texture.GetDesc(&mut dst_desc);

            if src_desc.Format != dst_desc.Format {
                tracing::error!(
                    "GPU Copy aborted! Format mismatch: {:?} vs {:?}",
                    src_desc.Format,
                    dst_desc.Format
                );
            } else {
                let src_box = windows::Win32::Graphics::Direct3D11::D3D11_BOX {
                    left: 0,
                    top: 0,
                    front: 0,
                    right: std::cmp::min(width as u32, dst_w),
                    bottom: std::cmp::min(height as u32, dst_h),
                    back: 1,
                };

                // Perform GPU-to-GPU copy from WGC texture to FFmpeg texture
                device_context.CopySubresourceRegion(
                    dst_texture,
                    dst_subresource,
                    0,
                    0,
                    0,
                    texture,
                    0,
                    Some(&src_box),
                );
            }

            if let Some(unlock) = (*d3d11_ctx).unlock {
                unlock((*d3d11_ctx).lock_ctx);
            }
        }

        input_frame.set_pts(Some(self.frame_count));
        self.frame_count += 1;

        encoder.send_frame(&input_frame).map_err(FFmpegEncoderError::Encode)
    }

    fn encode_software(
        &mut self,
        frame: &Frame,
        width: i32,
        height: i32,
        dst_w: u32,
        dst_h: u32,
        dst_format: format::Pixel,
    ) -> Result<()> {
        let encoder = self.encoder.as_mut().unwrap();
        let mut input_frame = ffmpeg::frame::Video::empty();

        let pixels = frame
            .get_software_pixels()
            .ok_or(FFmpegEncoderError::Conversion(ffmpeg::Error::InvalidData))?;

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

        encoder.send_frame(&dst_frame).map_err(FFmpegEncoderError::Encode)
    }

    fn collect_nal_units(&mut self) -> Result<Vec<Vec<u8>>> {
        let encoder = self.encoder.as_mut().unwrap();
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

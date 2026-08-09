use std::sync::Mutex;

use ffmpeg::{Codec, codec, frame, util::format};
use ffmpeg_next as ffmpeg;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D11::{
                D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEX2D_VPIV,
                D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
                D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
                D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
                D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
                D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Device,
                ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, ID3D11VideoContext,
                ID3D11VideoContext1, ID3D11VideoDevice, ID3D11VideoProcessor,
                ID3D11VideoProcessorEnumerator,
            },
            Dxgi::Common::{
                DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_TYPE,
                DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P601,
                DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P709,
                DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P2020,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P601,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P2020, DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_RATIONAL, DXGI_SAMPLE_DESC,
            },
        },
    },
    core::Interface,
};

use super::{
    super::{Error, Result},
    FrameOutput,
};
use crate::media::{
    Dimensions, PixelFormat,
    codec::backend::ffmpeg::D3d11vaDeviceContext,
    frame::{D3d11FrameProducer, D3d11FrameWriter, Frame},
};

#[derive(Default)]
struct OutputState {
    width: u32,
    height: u32,
    frame_producer: Option<D3d11FrameProducer>,
    video_enumerator: Option<ID3D11VideoProcessorEnumerator>,
    video_processor: Option<ID3D11VideoProcessor>,
}

struct OutputResources {
    frame_writer: D3d11FrameWriter,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
}

struct DeviceContextLock<'a> {
    context: &'a D3d11vaDeviceContext,
}

impl<'a> DeviceContextLock<'a> {
    unsafe fn new(context: &'a D3d11vaDeviceContext) -> Self {
        if let Some(lock) = context.lock {
            unsafe { lock(context.lock_ctx) };
        }
        Self { context }
    }
}

impl Drop for DeviceContextLock<'_> {
    fn drop(&mut self) {
        if let Some(unlock) = self.context.unlock {
            unsafe { unlock(self.context.lock_ctx) };
        }
    }
}

pub(super) struct Backend {
    device_ctx: *mut ffmpeg_next::ffi::AVBufferRef,
    output_state: Mutex<OutputState>,
}

impl Backend {
    pub(super) fn configure(codec: &Codec, context: &mut codec::Context) -> Option<Self> {
        if !Self::codec_supports_d3d11va(codec) {
            return None;
        }

        unsafe {
            let mut device_ctx = std::ptr::null_mut();
            let ret = ffmpeg_next::ffi::av_hwdevice_ctx_create(
                &mut device_ctx,
                ffmpeg_next::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );
            if ret < 0 || device_ctx.is_null() {
                tracing::warn!("Failed to create D3D11VA device context: {}", ret);
                return None;
            }

            let codec_context = context.as_mut_ptr();
            (*codec_context).get_format = Some(Self::get_d3d11_format);
            (*codec_context).extra_hw_frames = 1;
            (*codec_context).hw_device_ctx = ffmpeg_next::ffi::av_buffer_ref(device_ctx);

            if (*codec_context).hw_device_ctx.is_null() {
                tracing::warn!("Failed to retain D3D11VA device context for decoder");
                ffmpeg_next::ffi::av_buffer_unref(&mut device_ctx);
                return None;
            }

            Some(Self { device_ctx, output_state: Mutex::new(OutputState::default()) })
        }
    }

    pub(super) fn materialize_frame(&self, decoded_frame: frame::Video) -> Result<frame::Video> {
        if decoded_frame.format() == format::Pixel::D3D11 {
            let mut software_frame = frame::Video::empty();

            unsafe {
                let ret = ffmpeg_next::ffi::av_hwframe_transfer_data(
                    software_frame.as_mut_ptr(),
                    decoded_frame.as_ptr(),
                    0,
                );
                if ret < 0 {
                    return Err(Error::Conversion(ffmpeg::Error::from(ret)));
                }
            }

            return Ok(software_frame);
        }

        Ok(decoded_frame)
    }

    pub(super) fn try_decode_frame(
        &self,
        decoded_frame: &frame::Video,
        dst_format: PixelFormat,
    ) -> Result<FrameOutput> {
        if dst_format != PixelFormat::BGRA8 || decoded_frame.format() != format::Pixel::D3D11 {
            return Ok(FrameOutput::Unsupported);
        }

        unsafe {
            let raw_frame = decoded_frame.as_ptr();
            let texture_ptr = (*raw_frame).data[0] as *mut std::ffi::c_void;
            if texture_ptr.is_null() {
                return Ok(FrameOutput::Unsupported);
            }

            let src_subresource = (*raw_frame).data[1] as usize as u32;
            let src_texture =
                <ID3D11Texture2D as windows::core::Interface>::from_raw_borrowed(&texture_ptr)
                    .ok_or_else(|| {
                        Error::HardwareInterop("Failed to borrow decoded D3D11 texture".into())
                    })?;

            let hw_device_ctx = self.hw_device_context();
            let device_context = Self::borrow_interface::<ID3D11DeviceContext>(
                &hw_device_ctx.device_context,
                "D3D11VA device context",
            )?;
            let video_device = Self::borrow_interface::<ID3D11VideoDevice>(
                &hw_device_ctx.video_device,
                "D3D11VA video device",
            )?;
            let video_context = Self::borrow_interface::<ID3D11VideoContext>(
                &hw_device_ctx.video_context,
                "D3D11VA video context",
            )?;

            let visible_width = decoded_frame.width();
            let visible_height = decoded_frame.height();
            let Some(output_resources) =
                self.ensure_output_resources(video_device, visible_width, visible_height)?
            else {
                tracing::debug!("decoded GPU frame pool is full; dropping the newest output");
                return Ok(FrameOutput::Backpressured);
            };

            let src_resource: ID3D11Resource = src_texture.cast().map_err(|err| {
                Error::HardwareInterop(format!(
                    "Failed to cast source texture to resource: {}",
                    err
                ))
            })?;
            let dst_resource: ID3D11Resource =
                output_resources.frame_writer.texture().cast().map_err(|err| {
                    Error::HardwareInterop(format!(
                        "Failed to cast destination texture to resource: {}",
                        err
                    ))
                })?;

            let src_view_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: src_subresource },
                },
            };

            let dst_view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            };

            let mut input_view = None;
            video_device
                .CreateVideoProcessorInputView(
                    &src_resource,
                    &output_resources.enumerator,
                    &src_view_desc,
                    Some(&mut input_view),
                )
                .map_err(|err| {
                    Error::HardwareInterop(format!("Failed to create decoder input view: {}", err))
                })?;

            let mut output_view = None;
            video_device
                .CreateVideoProcessorOutputView(
                    &dst_resource,
                    &output_resources.enumerator,
                    &dst_view_desc,
                    Some(&mut output_view),
                )
                .map_err(|err| {
                    Error::HardwareInterop(format!("Failed to create decoder output view: {}", err))
                })?;

            let input_view = input_view.ok_or_else(|| {
                Error::HardwareInterop("Decoder input view creation returned no view".into())
            })?;
            let output_view = output_view.ok_or_else(|| {
                Error::HardwareInterop("Decoder output view creation returned no view".into())
            })?;

            let visible_rect = RECT {
                left: 0,
                top: 0,
                right: visible_width as i32,
                bottom: visible_height as i32,
            };

            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                OutputIndex: 0,
                InputFrameOrField: 0,
                PastFrames: 0,
                FutureFrames: 0,
                ppPastSurfaces: std::ptr::null_mut(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(input_view)),
                ppFutureSurfaces: std::ptr::null_mut(),
                ppPastSurfacesRight: std::ptr::null_mut(),
                pInputSurfaceRight: std::mem::ManuallyDrop::new(None),
                ppFutureSurfacesRight: std::ptr::null_mut(),
            };

            let publish_result = {
                // FFmpeg may use this immediate/video context concurrently.
                // Keep the complete conversion and ready-signal transaction
                // under its device lock so no work can interleave between the
                // blit and the fence value published with this frame.
                let _context_lock = DeviceContextLock::new(hw_device_ctx);
                Self::configure_color_space(
                    video_context,
                    &output_resources.processor,
                    decoded_frame,
                );
                video_context.VideoProcessorSetStreamSourceRect(
                    &output_resources.processor,
                    0,
                    true,
                    Some(&visible_rect),
                );
                video_context.VideoProcessorSetStreamDestRect(
                    &output_resources.processor,
                    0,
                    true,
                    Some(&visible_rect),
                );
                video_context.VideoProcessorSetOutputTargetRect(
                    &output_resources.processor,
                    true,
                    Some(&visible_rect),
                );
                let blt_result = video_context.VideoProcessorBlt(
                    &output_resources.processor,
                    &output_view,
                    0,
                    std::slice::from_ref(&stream),
                );

                // The generated stream type uses ManuallyDrop for its COM
                // input views and therefore does not release them itself.
                drop(std::mem::ManuallyDrop::into_inner(stream.pInputSurface));
                drop(std::mem::ManuallyDrop::into_inner(stream.pInputSurfaceRight));

                match blt_result {
                    Ok(()) => output_resources.frame_writer.finish(device_context).map_err(|err| {
                        Error::HardwareInterop(format!(
                            "Failed to publish decoded GPU frame: {err}"
                        ))
                    }),
                    Err(error) => {
                        output_resources.frame_writer.quarantine();
                        Err(Error::HardwareInterop(format!(
                            "Failed to convert decoded frame with D3D11 video processor: {error}"
                        )))
                    }
                }
            };
            let gpu_resource = match publish_result {
                Ok(resource) => resource,
                Err(error) => {
                    // Failed conversion or readiness quarantines the physical
                    // slot. Rebuild immediately so later frames neither reuse
                    // it nor consume capacity one poisoned slot at a time.
                    self.output_state.lock().unwrap().frame_producer = None;
                    return Err(error);
                }
            };

            Ok(FrameOutput::Ready(Frame::new_gpu(
                gpu_resource,
                None,
                PixelFormat::BGRA8,
                Dimensions::new(visible_width as i32, visible_height as i32),
                None,
            )))
        }
    }

    pub(super) fn name(&self) -> &'static str {
        "D3D11VA"
    }

    unsafe fn hw_device_context(&self) -> &D3d11vaDeviceContext {
        let hw_ctx = unsafe { (*self.device_ctx).data as *mut ffmpeg_next::ffi::AVHWDeviceContext };
        let d3d11_ctx = unsafe { (*hw_ctx).hwctx as *mut D3d11vaDeviceContext };
        unsafe { &*d3d11_ctx }
    }

    unsafe fn borrow_interface<'a, T: Interface>(
        raw: &'a *mut std::ffi::c_void,
        label: &str,
    ) -> Result<&'a T> {
        if raw.is_null() {
            return Err(Error::HardwareInterop(format!("{} pointer is null", label)));
        }

        unsafe { T::from_raw_borrowed(raw) }
            .ok_or_else(|| Error::HardwareInterop(format!("Failed to borrow {} interface", label)))
    }

    unsafe fn ensure_output_resources(
        &self,
        video_device: &ID3D11VideoDevice,
        width: u32,
        height: u32,
    ) -> Result<Option<OutputResources>> {
        if width == 0
            || height == 0
            || i32::try_from(width).is_err()
            || i32::try_from(height).is_err()
        {
            return Err(Error::HardwareInterop(format!(
                "Invalid decoded-frame dimensions {width}x{height}"
            )));
        }

        let mut state = self.output_state.lock().unwrap();
        if state.frame_producer.is_none() || state.width != width || state.height != height {
            let content_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                InputWidth: width,
                InputHeight: height,
                OutputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                OutputWidth: width,
                OutputHeight: height,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };

            let enumerator = unsafe { video_device.CreateVideoProcessorEnumerator(&content_desc) }
                .map_err(|err| {
                    Error::HardwareInterop(format!(
                        "Failed to create video processor enumerator: {}",
                        err
                    ))
                })?;
            let processor =
                unsafe { video_device.CreateVideoProcessor(&enumerator, 0) }.map_err(|err| {
                    Error::HardwareInterop(format!("Failed to create video processor: {}", err))
                })?;

            let hw_device_ctx = unsafe { self.hw_device_context() };
            let device = unsafe {
                Self::borrow_interface::<ID3D11Device>(&hw_device_ctx.device, "D3D11VA device")
            }?;
            let frame_producer = D3d11FrameProducer::new(device.clone()).map_err(|err| {
                Error::HardwareInterop(format!(
                    "Failed to create decoded-frame GPU timeline: {}",
                    err
                ))
            })?;

            state.width = width;
            state.height = height;
            state.frame_producer = Some(frame_producer);
            state.video_enumerator = Some(enumerator);
            state.video_processor = Some(processor);
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let frame_writer = state
            .frame_producer
            .as_mut()
            .expect("decoded-frame producer was initialized")
            .try_begin_frame(desc)
            .map_err(|err| {
                Error::HardwareInterop(format!(
                    "Failed to reserve a decoded-frame GPU texture: {}",
                    err
                ))
            })?;

        let Some(frame_writer) = frame_writer else {
            return Ok(None);
        };

        Ok(Some(OutputResources {
            frame_writer,
            enumerator: state.video_enumerator.as_ref().unwrap().clone(),
            processor: state.video_processor.as_ref().unwrap().clone(),
        }))
    }

    fn configure_color_space(
        video_context: &ID3D11VideoContext,
        processor: &ID3D11VideoProcessor,
        decoded_frame: &frame::Video,
    ) {
        let Ok(video_context1) = video_context.cast::<ID3D11VideoContext1>() else {
            return;
        };

        let input_space = Self::input_color_space(decoded_frame);

        unsafe {
            video_context1.VideoProcessorSetStreamColorSpace1(processor, 0, input_space);
            video_context1.VideoProcessorSetOutputColorSpace1(
                processor,
                DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
            );
        }
    }

    fn input_color_space(decoded_frame: &frame::Video) -> DXGI_COLOR_SPACE_TYPE {
        let matrix_is_bt2020 = matches!(
            decoded_frame.color_space(),
            ffmpeg::util::color::Space::BT2020NCL | ffmpeg::util::color::Space::BT2020CL
        );
        let matrix_is_bt601 = matches!(
            decoded_frame.color_space(),
            ffmpeg::util::color::Space::BT470BG | ffmpeg::util::color::Space::SMPTE170M
        );
        let full_range = matches!(decoded_frame.color_range(), ffmpeg::util::color::Range::JPEG);

        match (matrix_is_bt2020, matrix_is_bt601, full_range) {
            (true, _, true) => DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P2020,
            (true, _, false) => DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P2020,
            (_, true, true) => DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P601,
            (_, true, false) => DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P601,
            (_, _, true) => DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P709,
            (_, _, false) => DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
        }
    }

    fn codec_supports_d3d11va(codec: &Codec) -> bool {
        unsafe {
            let mut idx = 0;
            loop {
                let config = ffmpeg_next::ffi::avcodec_get_hw_config(codec.as_ptr(), idx);
                if config.is_null() {
                    return false;
                }

                let supports_device_ctx = ((*config).methods
                    & ffmpeg_next::ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32)
                    != 0;
                if supports_device_ctx
                    && (*config).device_type
                        == ffmpeg_next::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA
                    && (*config).pix_fmt == ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_D3D11
                {
                    return true;
                }

                idx += 1;
            }
        }
    }

    unsafe extern "C" fn get_d3d11_format(
        codec_context: *mut ffmpeg_next::ffi::AVCodecContext,
        pixel_formats: *const ffmpeg_next::ffi::AVPixelFormat,
    ) -> ffmpeg_next::ffi::AVPixelFormat {
        let mut current = pixel_formats;

        while !current.is_null()
            && unsafe { *current != ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_NONE }
        {
            if unsafe { *current == ffmpeg_next::ffi::AVPixelFormat::AV_PIX_FMT_D3D11 } {
                return unsafe { *current };
            }
            current = unsafe { current.add(1) };
        }

        unsafe { ffmpeg_next::ffi::avcodec_default_get_format(codec_context, pixel_formats) }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        unsafe {
            ffmpeg_next::ffi::av_buffer_unref(&mut self.device_ctx);
        }
    }
}

unsafe impl Send for Backend {}

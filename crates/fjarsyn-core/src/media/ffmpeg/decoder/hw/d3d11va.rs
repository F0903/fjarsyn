use std::sync::Mutex;

use ffmpeg::{Codec, codec, format, frame};
use ffmpeg_next as ffmpeg;
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_SHARED,
        D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
        D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
        D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
        D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
        D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
        ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoContext1, ID3D11VideoDevice,
        ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    },
    Dxgi::{
        Common::{
            DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_TYPE,
            DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P601, DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P709,
            DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P2020,
            DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P601,
            DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
            DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P2020, DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_RATIONAL,
        },
        IDXGIResource1,
    },
};
use windows_core::Interface;

use super::super::{FFmpegDecoderError, Result};
use crate::{
    media::{
        frame::{Frame, GpuFrameResource, GpuImportHandle},
        pixel_format::PixelFormat,
    },
    utils::vector2::Vector2,
};

#[repr(C)]
struct AVD3D11VADeviceContext {
    device: *mut std::ffi::c_void,
    device_context: *mut std::ffi::c_void,
    video_device: *mut std::ffi::c_void,
    video_context: *mut std::ffi::c_void,
    lock: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void)>,
    unlock: Option<unsafe extern "C" fn(ctx: *mut std::ffi::c_void)>,
    lock_ctx: *mut std::ffi::c_void,
}

#[derive(Default)]
struct SharedTexturePool {
    width: u32,
    height: u32,
    frame_count: u64,
    textures: Vec<ID3D11Texture2D>,
    handles: Vec<GpuImportHandle>,
    video_enumerator: Option<ID3D11VideoProcessorEnumerator>,
    video_processor: Option<ID3D11VideoProcessor>,
}

struct OutputResources {
    write_texture: ID3D11Texture2D,
    completed_texture: ID3D11Texture2D,
    completed_handle: GpuImportHandle,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
}

pub(super) struct D3d11vaBackend {
    device_ctx: *mut ffmpeg_next::ffi::AVBufferRef,
    output_pool: Mutex<SharedTexturePool>,
}

impl D3d11vaBackend {
    const OUTPUT_POOL_SIZE: usize = 3;

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

            Some(Self { device_ctx, output_pool: Mutex::new(SharedTexturePool::default()) })
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
                    return Err(FFmpegDecoderError::Conversion(ffmpeg::Error::from(ret)));
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
    ) -> Result<Option<Frame>> {
        if dst_format != PixelFormat::BGRA8 || decoded_frame.format() != format::Pixel::D3D11 {
            return Ok(None);
        }

        unsafe {
            let raw_frame = decoded_frame.as_ptr();
            let texture_ptr = (*raw_frame).data[0] as *mut std::ffi::c_void;
            if texture_ptr.is_null() {
                return Ok(None);
            }

            let src_subresource = (*raw_frame).data[1] as usize as u32;
            let src_texture =
                <ID3D11Texture2D as windows_core::Interface>::from_raw_borrowed(&texture_ptr)
                    .ok_or_else(|| {
                        FFmpegDecoderError::HardwareInterop(
                            "Failed to borrow decoded D3D11 texture".into(),
                        )
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

            let mut src_desc = std::mem::zeroed::<D3D11_TEXTURE2D_DESC>();
            src_texture.GetDesc(&mut src_desc);

            let output_resources = self.ensure_output_resources(video_device, &src_desc)?;

            let src_resource: ID3D11Resource = src_texture.cast().map_err(|err| {
                FFmpegDecoderError::HardwareInterop(format!(
                    "Failed to cast source texture to resource: {}",
                    err
                ))
            })?;
            let dst_resource: ID3D11Resource =
                output_resources.write_texture.cast().map_err(|err| {
                    FFmpegDecoderError::HardwareInterop(format!(
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
                    FFmpegDecoderError::HardwareInterop(format!(
                        "Failed to create decoder input view: {}",
                        err
                    ))
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
                    FFmpegDecoderError::HardwareInterop(format!(
                        "Failed to create decoder output view: {}",
                        err
                    ))
                })?;

            let input_view = input_view.ok_or_else(|| {
                FFmpegDecoderError::HardwareInterop(
                    "Decoder input view creation returned no view".into(),
                )
            })?;
            let output_view = output_view.ok_or_else(|| {
                FFmpegDecoderError::HardwareInterop(
                    "Decoder output view creation returned no view".into(),
                )
            })?;

            Self::configure_color_space(video_context, &output_resources.processor, decoded_frame);

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

            self.lock(hw_device_ctx);
            let blt_result = video_context.VideoProcessorBlt(
                &output_resources.processor,
                &output_view,
                0,
                &[stream],
            );
            self.unlock(hw_device_ctx);

            blt_result.map_err(|err| {
                FFmpegDecoderError::HardwareInterop(format!(
                    "Failed to convert decoded frame with D3D11 video processor: {}",
                    err
                ))
            })?;

            device_context.Flush();

            Ok(Some(Frame::new_gpu(
                // Expose the most recently completed shared texture instead of
                // the surface we just wrote into this decode step.
                GpuFrameResource::D3D11Texture(output_resources.completed_texture.clone()),
                Some(output_resources.completed_handle),
                None,
                None,
                PixelFormat::BGRA8,
                Vector2::new(decoded_frame.width() as i32, decoded_frame.height() as i32),
                None,
            )))
        }
    }

    pub(super) fn name(&self) -> &'static str {
        "D3D11VA"
    }

    unsafe fn hw_device_context(&self) -> &AVD3D11VADeviceContext {
        let hw_ctx = unsafe { (*self.device_ctx).data as *mut ffmpeg_next::ffi::AVHWDeviceContext };
        let d3d11_ctx = unsafe { (*hw_ctx).hwctx as *mut AVD3D11VADeviceContext };
        unsafe { &*d3d11_ctx }
    }

    unsafe fn borrow_interface<'a, T: Interface>(
        raw: &'a *mut std::ffi::c_void,
        label: &str,
    ) -> Result<&'a T> {
        if raw.is_null() {
            return Err(FFmpegDecoderError::HardwareInterop(format!("{} pointer is null", label)));
        }

        unsafe { T::from_raw_borrowed(raw) }.ok_or_else(|| {
            FFmpegDecoderError::HardwareInterop(format!("Failed to borrow {} interface", label))
        })
    }

    unsafe fn ensure_output_resources(
        &self,
        video_device: &ID3D11VideoDevice,
        src_desc: &D3D11_TEXTURE2D_DESC,
    ) -> Result<OutputResources> {
        let mut pool = self.output_pool.lock().unwrap();
        if pool.textures.is_empty()
            || pool.width != src_desc.Width
            || pool.height != src_desc.Height
        {
            let content_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                InputWidth: src_desc.Width,
                InputHeight: src_desc.Height,
                OutputFrameRate: DXGI_RATIONAL { Numerator: 60, Denominator: 1 },
                OutputWidth: src_desc.Width,
                OutputHeight: src_desc.Height,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };

            let enumerator = unsafe { video_device.CreateVideoProcessorEnumerator(&content_desc) }
                .map_err(|err| {
                    FFmpegDecoderError::HardwareInterop(format!(
                        "Failed to create video processor enumerator: {}",
                        err
                    ))
                })?;
            let processor =
                unsafe { video_device.CreateVideoProcessor(&enumerator, 0) }.map_err(|err| {
                    FFmpegDecoderError::HardwareInterop(format!(
                        "Failed to create video processor: {}",
                        err
                    ))
                })?;

            pool.width = src_desc.Width;
            pool.height = src_desc.Height;
            pool.frame_count = 0;
            pool.textures.clear();
            pool.handles.clear();
            pool.video_enumerator = Some(enumerator);
            pool.video_processor = Some(processor);

            let hw_device_ctx = unsafe { self.hw_device_context() };
            let device = unsafe {
                Self::borrow_interface::<ID3D11Device>(&hw_device_ctx.device, "D3D11VA device")
            }?;

            for _ in 0..Self::OUTPUT_POOL_SIZE {
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: src_desc.Width,
                    Height: src_desc.Height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: src_desc.SampleDesc,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: (D3D11_RESOURCE_MISC_SHARED.0
                        | D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0)
                        as u32,
                };

                let texture = {
                    let mut texture = None;
                    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.map_err(
                        |err| {
                            FFmpegDecoderError::HardwareInterop(format!(
                                "Failed to create shared decoder texture: {}",
                                err
                            ))
                        },
                    )?;
                    texture.ok_or_else(|| {
                        FFmpegDecoderError::HardwareInterop(
                            "Decoder texture creation returned no texture".into(),
                        )
                    })?
                };

                let dxgi_resource: IDXGIResource1 = texture.cast().map_err(|err| {
                    FFmpegDecoderError::HardwareInterop(format!(
                        "Failed to cast decoder texture to IDXGIResource1: {}",
                        err
                    ))
                })?;

                let handle = unsafe {
                    dxgi_resource.CreateSharedHandle(
                        None,
                        windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_READ.0
                            | windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_WRITE.0,
                        None,
                    )
                }
                .map_err(|err| {
                    FFmpegDecoderError::HardwareInterop(format!(
                        "Failed to create shared decoder texture handle: {}",
                        err
                    ))
                })?;

                pool.textures.push(texture);
                pool.handles.push(GpuImportHandle::from_windows_nt_handle(handle));
            }
        }

        let pool_len = pool.textures.len() as u64;
        let write_index = (pool.frame_count % pool_len) as usize;
        let preview_index = if pool.frame_count > 0 {
            ((pool.frame_count - 1) % pool_len) as usize
        } else {
            write_index
        };
        pool.frame_count += 1;

        Ok(OutputResources {
            write_texture: pool.textures[write_index].clone(),
            completed_texture: pool.textures[preview_index].clone(),
            completed_handle: pool.handles[preview_index],
            enumerator: pool.video_enumerator.as_ref().unwrap().clone(),
            processor: pool.video_processor.as_ref().unwrap().clone(),
        })
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

    unsafe fn lock(&self, hw_device_ctx: &AVD3D11VADeviceContext) {
        if let Some(lock) = hw_device_ctx.lock {
            unsafe { lock(hw_device_ctx.lock_ctx) };
        }
    }

    unsafe fn unlock(&self, hw_device_ctx: &AVD3D11VADeviceContext) {
        if let Some(unlock) = hw_device_ctx.unlock {
            unsafe { unlock(hw_device_ctx.lock_ctx) };
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

impl Drop for D3d11vaBackend {
    fn drop(&mut self) {
        unsafe {
            ffmpeg_next::ffi::av_buffer_unref(&mut self.device_ctx);
        }
    }
}

unsafe impl Send for D3d11vaBackend {}

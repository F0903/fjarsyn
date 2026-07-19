use ffmpeg::{codec, format};
use ffmpeg_next as ffmpeg;
use windows_core::Interface;

use super::{FFmpegEncoder, FFmpegEncoderError, Result};
use crate::media::CodecDeviceLease;

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

impl FFmpegEncoder {
    pub(super) fn init_hw_device_ctx(
        device_lease: &CodecDeviceLease,
    ) -> Option<*mut ffmpeg_next::ffi::AVBufferRef> {
        unsafe {
            let ctx = ffmpeg_next::ffi::av_hwdevice_ctx_alloc(
                ffmpeg_next::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            );
            if !ctx.is_null() {
                let hw_ctx = (*ctx).data as *mut ffmpeg_next::ffi::AVHWDeviceContext;
                let d3d11_ctx = (*hw_ctx).hwctx as *mut AVD3D11VADeviceContext;

                // Transfer one explicit COM reference for each interface that
                // FFmpeg's D3D11 device context will release.
                let device = std::mem::ManuallyDrop::new(device_lease.d3d11().clone());
                let device_context = device.GetImmediateContext().ok();

                (*d3d11_ctx).device = device.as_raw() as *mut _;
                if let Some(context) = device_context {
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

    pub(super) fn init_hw_frames_ctx(
        &self,
        codec_context: &mut codec::encoder::video::Video,
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
                    codec_context.set_format(format::Pixel::D3D11);
                } else {
                    tracing::error!("Failed to initialize hardware frames context: {}", ret);
                }
                ffmpeg_next::ffi::av_buffer_unref(&mut frames_ctx_ref);
            }
        }
    }

    pub(super) fn encode_d3d11(
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

            let ret =
                ffmpeg_next::ffi::av_hwframe_get_buffer(hw_frames_ctx, input_frame.as_mut_ptr(), 0);
            if ret < 0 {
                tracing::error!("av_hwframe_get_buffer failed: {}", ret);
                return Err(FFmpegEncoderError::Encode(ffmpeg::Error::InvalidData));
            }

            let dst_texture_ptr = (*input_frame.as_mut_ptr()).data[0] as *mut std::ffi::c_void;
            let dst_subresource = (*input_frame.as_mut_ptr()).data[1] as usize as u32;

            if dst_texture_ptr.is_null() {
                tracing::error!("Allocated hardware frame has null texture pointer");
                return Err(FFmpegEncoderError::Encode(ffmpeg::Error::InvalidData));
            }

            let dst_texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D =
                std::mem::transmute(&dst_texture_ptr);

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
}

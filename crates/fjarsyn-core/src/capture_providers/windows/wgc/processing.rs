use std::sync::{Arc, RwLock};

use windows::{
    Graphics::Capture::Direct3D11CaptureFrame,
    Win32::{
        Graphics::Direct3D11::{D3D11_CPU_ACCESS_READ, D3D11_TEXTURE2D_DESC, ID3D11Texture2D},
        System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
    },
};
use windows_core::Interface;

use super::{ResourcePool, Result, WgcCaptureProvider};
use crate::{
    capture_providers::windows::{
        WindowsCaptureError,
        d3d11_utils::{copy_texture, map_read_texture},
    },
    media::{
        frame::{Frame, GpuFrameResource},
        pixel_format::PixelFormat,
    },
    utils::{buffer_pool::Buffer, vector2::Vector2},
};

impl WgcCaptureProvider {
    pub(super) fn process_frame(
        mut frame_buffer: Option<Buffer>,
        capture_frame: Direct3D11CaptureFrame,
        resource_state_arc: Arc<RwLock<ResourcePool>>,
        pixel_format: PixelFormat,
        target_frame_duration: std::time::Duration,
        tx: tokio::sync::mpsc::Sender<Frame>,
    ) -> Result<()> {
        let rel_time = capture_frame
            .SystemRelativeTime()
            .map_err(|e| {
                tracing::warn!("Failed to get frame system relative time: {}", e);
                e
            })
            .unwrap_or_default();

        let surface = capture_frame.Surface().map_err(|e| {
            tracing::error!("Failed to get surface! {}", e);
            WindowsCaptureError::FailedToGetSurface(e)
        })?;

        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| {
            tracing::error!("Failed to cast surface to access! {}", e);
            WindowsCaptureError::CastFailed(e)
        })?;

        let texture: ID3D11Texture2D = unsafe {
            access.GetInterface().map_err(|e| {
                tracing::error!("Failed to get interface! {}", e);
                WindowsCaptureError::FailedToGetInterface(e)
            })?
        };

        let size = capture_frame.ContentSize().map_err(|e| {
            tracing::error!("Failed to get frame ContentSize! {}", e);
            WindowsCaptureError::FailedToGetContentSize(e)
        })?;

        let device = unsafe {
            texture.GetDevice().map_err(|e| {
                tracing::error!("Failed to get device: {}", e);
                WindowsCaptureError::FailedToGetDevice(e)
            })?
        };

        let context = unsafe {
            device.GetImmediateContext().map_err(|e| {
                tracing::error!("Failed to get immediate context: {}", e);
                WindowsCaptureError::FailedToGetImmediateContext(e)
            })?
        };

        let desc = unsafe {
            let mut description = std::mem::zeroed::<D3D11_TEXTURE2D_DESC>();
            texture.GetDesc(&mut description);
            description
        };

        let mut pool =
            Self::ensure_resource_pool(&device, &resource_state_arc, desc, frame_buffer.is_some())?;
        let write_idx = (pool.frame_count % Self::PIPELINE_DEPTH as u64) as usize;

        let shared_texture = &pool.shared_textures[write_idx];
        copy_texture(&context, &texture, shared_texture);
        unsafe { context.Flush() };

        if frame_buffer.is_some() {
            let staging_texture = &pool.staging_textures[write_idx];
            copy_texture(&context, &texture, staging_texture);
        }

        let pipeline_primed = pool.frame_count > 0;
        let read_idx = if pipeline_primed {
            (pool.frame_count.wrapping_sub(1)) as usize % Self::PIPELINE_DEPTH
        } else {
            write_idx
        };

        let shared_handle = pool.shared_handles[read_idx];

        if let Some(buffer) = &mut frame_buffer {
            let read_staging_texture = &pool.staging_textures[read_idx];
            let mut staging_desc = desc;
            staging_desc.Usage = windows::Win32::Graphics::Direct3D11::D3D11_USAGE_STAGING;
            staging_desc.BindFlags = 0;
            staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            staging_desc.MiscFlags = 0;

            map_read_texture(
                buffer,
                &context,
                read_staging_texture,
                &staging_desc,
                pixel_format.bytes_per_pixel(),
            )?;
        }

        pool.frame_count += 1;

        let frame_duration = match pool.last_emitted_timestamp_100ns {
            Some(previous) if rel_time.Duration > previous => {
                std::time::Duration::from_nanos(((rel_time.Duration - previous) * 100) as u64)
            }
            _ => target_frame_duration,
        };
        pool.last_emitted_timestamp_100ns = Some(rel_time.Duration);

        let resource_owner: Arc<dyn std::any::Any + Send + Sync> = Arc::new(capture_frame);

        let output_frame = Frame::new_gpu(
            GpuFrameResource::D3D11Texture(texture.clone()),
            Some(shared_handle),
            frame_buffer,
            Some(resource_owner),
            pixel_format,
            Vector2 { x: size.Width, y: size.Height },
            Some(frame_duration),
        );

        match tx.try_send(output_frame) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                return Err(WindowsCaptureError::FrameSenderClosed);
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!("Frame channel full, dropping frame.");
            }
        }

        Ok(())
    }
}

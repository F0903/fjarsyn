use std::sync::{Arc, RwLock};

use windows::{
    Graphics::Capture::Direct3D11CaptureFrame,
    Win32::{
        Graphics::Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT, ID3D11Texture2D,
        },
        System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
    },
    core::Interface,
};

use super::{DeviceState, PIPELINE_DEPTH, ResourcePool};
use crate::media::{
    Dimensions, PixelFormat,
    buffer_pool::Buffer,
    capture::windows::{Error, Result, copy_texture, map_read_texture},
    frame::Frame,
};

pub(in crate::media::capture::windows::wgc) fn process_frame(
    mut frame_buffer: Option<Buffer>,
    capture_frame: Direct3D11CaptureFrame,
    current_device: &DeviceState,
    resources: Arc<RwLock<ResourcePool>>,
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
        Error::FailedToGetSurface(e)
    })?;

    let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| {
        tracing::error!("Failed to cast surface to access! {}", e);
        Error::CastFailed(e)
    })?;

    let texture: ID3D11Texture2D = unsafe {
        access.GetInterface().map_err(|e| {
            tracing::error!("Failed to get interface! {}", e);
            Error::FailedToGetInterface(e)
        })?
    };

    let size = capture_frame.ContentSize().map_err(|e| {
        tracing::error!("Failed to get frame ContentSize! {}", e);
        Error::FailedToGetContentSize(e)
    })?;

    let device = unsafe {
        texture.GetDevice().map_err(|e| {
            tracing::error!("Failed to get device: {}", e);
            Error::FailedToGetDevice(e)
        })?
    };

    let context = unsafe {
        device.GetImmediateContext().map_err(|e| {
            tracing::error!("Failed to get immediate context: {}", e);
            Error::FailedToGetImmediateContext(e)
        })?
    };

    let desc = unsafe {
        let mut description = std::mem::zeroed::<D3D11_TEXTURE2D_DESC>();
        texture.GetDesc(&mut description);
        description
    };

    // Retain the current-device read lease through publication. Recovery must
    // replace the device under its write lock, so a frame cannot become stale
    // between this identity check and entering the output channel.
    let current_device = current_device.current();
    let mut pool = resources.write().unwrap();
    if !current_device.matches_native(&device).map_err(Error::FailedToGetDevice)? {
        tracing::debug!("dropping a capture frame produced by a superseded D3D11 device");
        return Ok(());
    }
    pool.ensure(&device, desc, frame_buffer.is_some())?;
    let write_idx = (pool.frame_count % PIPELINE_DEPTH as u64) as usize;

    if frame_buffer.is_some() {
        let staging_texture = &pool.staging_textures[write_idx];
        copy_texture(&context, &texture, staging_texture);
    }

    let mut shared_desc = desc;
    shared_desc.Usage = D3D11_USAGE_DEFAULT;
    shared_desc.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
    shared_desc.CPUAccessFlags = 0;
    let gpu_resource = match pool.frame_producer.as_mut() {
        Some(producer) => match producer.try_begin_frame(shared_desc) {
            Ok(Some(writer)) => {
                copy_texture(&context, &texture, writer.texture());
                writer.finish(&context).map(Some)
            }
            Ok(None) => {
                tracing::debug!("GPU frame pool is full; dropping GPU export for this frame");
                Ok(None)
            }
            Err(error) => Err(error),
        },
        None => Ok(None),
    };
    let gpu_resource = match gpu_resource {
        Ok(resource) => resource,
        Err(error) if Error::is_recoverable_device_loss_error(&error) => {
            pool.frame_producer = None;
            return Err(error.into());
        }
        Err(error) if frame_buffer.is_some() => {
            tracing::warn!(
                %error,
                "GPU frame export failed; continuing with requested CPU readback"
            );
            pool.frame_producer = None;
            None
        }
        Err(error) => {
            pool.frame_producer = None;
            return Err(error.into());
        }
    };

    if gpu_resource.is_none() && frame_buffer.is_none() {
        return Ok(());
    }

    if let Some(buffer) = &mut frame_buffer {
        let read_staging_texture = &pool.staging_textures[write_idx];
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

    let output_frame = build_output_frame(
        gpu_resource,
        frame_buffer,
        pixel_format,
        Dimensions { width: size.Width, height: size.Height },
        Some(frame_duration),
    );

    match tx.try_send(output_frame) {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            return Err(Error::FrameSenderClosed);
        }
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            tracing::debug!("Frame channel full, dropping frame.");
        }
    }

    Ok(())
}

fn build_output_frame(
    gpu_resource: Option<Arc<crate::media::frame::GpuResource>>,
    frame_buffer: Option<Buffer>,
    pixel_format: PixelFormat,
    dimensions: Dimensions<i32>,
    duration: Option<std::time::Duration>,
) -> Frame {
    match gpu_resource {
        Some(resource) => {
            Frame::new_gpu(resource, frame_buffer, pixel_format, dimensions, duration)
        }
        None => Frame::new_software(
            frame_buffer.expect("CPU fallback was requested when GPU export was unavailable"),
            pixel_format,
            dimensions,
            duration,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::buffer_pool::Pool;

    #[test]
    fn requested_readback_publishes_software_frame_without_gpu_export() {
        let pool = Pool::new(4, 1);
        let mut buffer = pool.get(4);
        buffer.copy_from_slice(&[10, 20, 30, 255]);

        let frame =
            build_output_frame(None, Some(buffer), PixelFormat::BGRA8, Dimensions::new(1, 1), None);

        assert!(frame.gpu().is_none());
        assert_eq!(frame.format, PixelFormat::RGBA8);
        assert_eq!(frame.software_pixels().unwrap().as_ref(), &[30, 20, 10, 255]);
    }
}

use wgpu::hal::api::Dx12;
use windows::{
    Win32::Graphics::{
        Direct3D12::{
            D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
            ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource,
        },
        Dxgi::Common::{
            DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
        },
    },
    core::{IUnknown, Interface},
};

use super::{ImportError, ImportedFrameDrawGuard, ImportedFrameTexture};
use crate::media::{Dimensions, PixelFormat, frame::Frame};

pub(super) fn supports_zero_copy_preview(format: PixelFormat) -> bool {
    format.supports_zero_copy_preview()
}

pub(super) fn import_frame_texture(
    device: &wgpu::Device,
    frame: &Frame,
) -> Result<ImportedFrameTexture, ImportError> {
    let gpu = frame.gpu().ok_or(ImportError::NoGpuResource)?;
    let source = gpu.resource().clone();
    let native = source.windows();
    let (format, dxgi_format) =
        texture_formats_for(frame.format).ok_or(ImportError::UnsupportedFormat(frame.format))?;
    let (width, height) = valid_dimensions(frame.size)?;
    // SAFETY: The resource and fence handles were created for the D3D11
    // producer on this adapter. The returned wgpu texture and fence own the
    // opened D3D12 objects; each draw separately retains its exact frame lease
    // until the containing command buffer completes.
    unsafe {
        let hal_device = device.as_hal::<Dx12>().ok_or(ImportError::UnsupportedBackend)?;
        let raw_device = hal_device.raw_device().clone();

        let mut raw_resource: Option<ID3D12Resource> = None;
        raw_device
            .OpenSharedHandle(native.shared_handle(), &mut raw_resource)
            .map_err(ImportError::OpenTexture)?;
        let raw_resource = raw_resource.ok_or_else(|| {
            ImportError::DescriptorMismatch("opening the texture returned no resource".into())
        })?;
        validate_resource(&raw_resource, dxgi_format, width, height)?;

        let mut ready_fence: Option<ID3D12Fence> = None;
        raw_device
            .OpenSharedHandle(native.ready_fence_handle(), &mut ready_fence)
            .map_err(ImportError::OpenFence)?;
        let ready_fence = ready_fence.ok_or_else(|| {
            ImportError::DescriptorMismatch("opening the producer fence returned no fence".into())
        })?;

        let hal_texture = wgpu::hal::dx12::Device::texture_from_raw(
            raw_resource,
            format,
            wgpu::TextureDimension::D2,
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            1,
            1,
        );
        drop(hal_device);

        let texture_desc = wgpu::TextureDescriptor {
            label: Some("Imported Pooled Frame Texture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let texture = device.create_texture_from_hal::<Dx12>(hal_texture, &texture_desc);

        Ok(ImportedFrameTexture {
            texture,
            texture_id: source.texture_id(),
            ready_fence,
            device: raw_device,
        })
    }
}

pub(super) fn prepare_draw(
    imported: &ImportedFrameTexture,
    queue: &wgpu::Queue,
    frame: &Frame,
) -> Result<ImportedFrameDrawGuard, ImportError> {
    let gpu = frame.gpu().ok_or(ImportError::NoGpuResource)?;
    if gpu.texture_id() != imported.texture_id {
        return Err(ImportError::FrameTextureMismatch);
    }
    let source = gpu.resource().clone();
    let ready_value = source.windows().ready_value();

    // SAFETY: the queue/device identity is checked before touching the raw
    // queue. Queue::Wait is enqueued during the actual primitive draw, before
    // Iced submits the command buffer that samples this texture.
    unsafe {
        let hal_queue = queue.as_hal::<Dx12>().ok_or(ImportError::UnsupportedBackend)?;
        validate_device_queue(&imported.device, hal_queue.as_raw())?;

        // Duplicate waits are harmless and avoid a reservation race where one
        // caller records a value before its native Wait has actually succeeded.
        hal_queue
            .as_raw()
            .Wait(&imported.ready_fence, ready_value)
            .map_err(ImportError::WaitForProducer)?;
    }

    Ok(ImportedFrameDrawGuard { _source: source, _ready_fence: imported.ready_fence.clone() })
}

fn validate_device_queue(
    device: &ID3D12Device,
    queue: &ID3D12CommandQueue,
) -> Result<(), ImportError> {
    let mut queue_device = None;
    unsafe { queue.GetDevice(&mut queue_device) }.map_err(ImportError::InspectQueueDevice)?;
    let queue_device: ID3D12Device = queue_device
        .ok_or_else(|| ImportError::InspectQueueDevice(windows::core::Error::empty()))?;
    let device_identity: IUnknown = device.cast().map_err(ImportError::InspectQueueDevice)?;
    let queue_identity: IUnknown = queue_device.cast().map_err(ImportError::InspectQueueDevice)?;

    if device_identity.as_raw() != queue_identity.as_raw() {
        return Err(ImportError::DeviceQueueMismatch);
    }
    Ok(())
}

fn valid_dimensions(size: Dimensions<i32>) -> Result<(u32, u32), ImportError> {
    let width = u32::try_from(size.width)
        .map_err(|_| ImportError::InvalidDimensions { width: size.width, height: size.height })?;
    let height = u32::try_from(size.height)
        .map_err(|_| ImportError::InvalidDimensions { width: size.width, height: size.height })?;
    if width == 0 || height == 0 {
        return Err(ImportError::InvalidDimensions { width: size.width, height: size.height });
    }
    Ok((width, height))
}

fn validate_resource(
    resource: &ID3D12Resource,
    format: DXGI_FORMAT,
    width: u32,
    height: u32,
) -> Result<(), ImportError> {
    let desc = unsafe { resource.GetDesc() };
    let simultaneous_access = desc.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS.0 != 0;
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.Width != u64::from(width)
        || desc.Height != height
        || desc.DepthOrArraySize != 1
        || desc.MipLevels != 1
        || desc.SampleDesc.Count != 1
        || desc.Format != format
        || !simultaneous_access
    {
        return Err(ImportError::DescriptorMismatch(format!(
            "expected a {width}x{height} single-sample {format:?} simultaneous-access Texture2D, got {desc:?}"
        )));
    }
    Ok(())
}

fn texture_formats_for(format: PixelFormat) -> Option<(wgpu::TextureFormat, DXGI_FORMAT)> {
    match format {
        PixelFormat::BGRA8 => Some((wgpu::TextureFormat::Bgra8Unorm, DXGI_FORMAT_B8G8R8A8_UNORM)),
        PixelFormat::RGBA8 => Some((wgpu::TextureFormat::Rgba8Unorm, DXGI_FORMAT_R8G8B8A8_UNORM)),
        PixelFormat::RGBA16 => {
            Some((wgpu::TextureFormat::Rgba16Float, DXGI_FORMAT_R16G16B16A16_FLOAT))
        }
        PixelFormat::RGBA10 => {
            Some((wgpu::TextureFormat::Rgb10a2Unorm, DXGI_FORMAT_R10G10B10A2_UNORM))
        }
        PixelFormat::NV12 => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_must_be_positive_before_native_import() {
        assert_eq!(valid_dimensions(Dimensions::new(1, 1)).unwrap(), (1, 1));

        for size in [
            Dimensions::new(0, 1),
            Dimensions::new(1, 0),
            Dimensions::new(-1, 1),
            Dimensions::new(1, -1),
        ] {
            assert!(matches!(
                valid_dimensions(size),
                Err(ImportError::InvalidDimensions { width, height })
                    if width == size.width && height == size.height
            ));
        }
    }
}

use wgpu::hal::api::Dx12;
use windows::Win32::Graphics::Direct3D12 as d3d12;

use super::ImportedFrameTexture;
use crate::media::{
    frame::{Frame, GpuImportHandle},
    pixel_format::PixelFormat,
};

pub fn import_frame_texture(device: &wgpu::Device, frame: &Frame) -> Option<ImportedFrameTexture> {
    let GpuImportHandle::WindowsNtHandle(handle) = frame.gpu_import_handle()?;
    let format = texture_format_for(frame.format)?;
    let width = frame.size.x as u32;
    let height = frame.size.y as u32;

    unsafe {
        let hal_device = device.as_hal::<Dx12>()?;

        let raw_device = hal_device.raw_device();
        let mut raw_resource: Option<d3d12::ID3D12Resource> = None;

        if let Err(e) = raw_device.OpenSharedHandle(handle, &mut raw_resource) {
            tracing::error!("Failed to open shared handle: {}", e);
            return None;
        }

        let raw_resource = raw_resource.unwrap();
        let hal_texture = wgpu::hal::dx12::Device::texture_from_raw(
            raw_resource,
            format,
            wgpu::TextureDimension::D2,
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            1,
            1,
        );

        let texture_desc = wgpu::TextureDescriptor {
            label: Some("Imported Shared Texture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        let texture = device.create_texture_from_hal::<Dx12>(hal_texture, &texture_desc);

        Some(ImportedFrameTexture { texture })
    }
}

fn texture_format_for(format: PixelFormat) -> Option<wgpu::TextureFormat> {
    match format {
        PixelFormat::BGRA8 => Some(wgpu::TextureFormat::Bgra8Unorm),
        PixelFormat::RGBA8 => Some(wgpu::TextureFormat::Rgba8Unorm),
        PixelFormat::RGBA16 => Some(wgpu::TextureFormat::Rgba16Float),
        PixelFormat::RGBA10 => Some(wgpu::TextureFormat::Rgb10a2Unorm),
        PixelFormat::NV12 => None,
    }
}

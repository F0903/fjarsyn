//! CPU- and GPU-backed video frame data.

use std::{sync::Arc, time::Duration};

use crate::media::{Dimensions, PixelFormat, buffer_pool::Buffer};

mod gpu_resource;

pub(crate) use gpu_resource::GpuResource;
#[cfg(target_os = "windows")]
pub(crate) use gpu_resource::{D3d11FrameProducer, D3d11FrameWriter};
pub use gpu_resource::{GpuFrameId, GpuTextureId};

#[inline]
fn ensure_rgba8(bitmap: &mut [u8], format: &mut PixelFormat) {
    match format {
        PixelFormat::RGBA8 => {}
        PixelFormat::BGRA8 => {
            for pixel in bitmap.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            *format = PixelFormat::RGBA8;
        }
        PixelFormat::RGBA16 | PixelFormat::RGBA10 | PixelFormat::NV12 => {
            tracing::debug!("Skipping unsupported software preview conversion for {format:?}");
        }
    }
}

#[derive(Debug)]
pub struct GpuFrameData {
    resource: Arc<GpuResource>,
    mapped_buffer: Option<bytes::Bytes>,
}

impl GpuFrameData {
    pub(crate) fn new(resource: Arc<GpuResource>, mapped_buffer: Option<Buffer>) -> Self {
        Self { resource, mapped_buffer: mapped_buffer.map(Buffer::freeze) }
    }

    pub fn frame_id(&self) -> GpuFrameId {
        self.resource.frame_id()
    }

    pub fn texture_id(&self) -> GpuTextureId {
        self.resource.texture_id()
    }

    pub(crate) fn resource(&self) -> &Arc<GpuResource> {
        &self.resource
    }

    pub fn software_pixels(&self) -> Option<bytes::Bytes> {
        self.mapped_buffer.clone()
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn d3d11_texture(&self) -> &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D {
        self.resource.windows().texture()
    }
}

#[derive(Debug)]
pub enum FrameData {
    /// CPU-accessible memory buffer.
    Software(bytes::Bytes),

    /// One leased GPU texture publication with optional retained CPU fallback pixels.
    Gpu(GpuFrameData),
}

#[derive(Debug)]
pub struct Frame {
    pub data: FrameData,
    pub format: PixelFormat,
    pub size: Dimensions<i32>,
    pub duration: Option<Duration>,
}

impl Frame {
    pub(crate) fn new_software(
        mut data: Buffer,
        mut format: PixelFormat,
        size: Dimensions<i32>,
        duration: Option<Duration>,
    ) -> Self {
        ensure_rgba8(&mut data, &mut format);
        Self { data: FrameData::Software(data.freeze()), format, size, duration }
    }

    pub(crate) fn new_gpu(
        resource: Arc<GpuResource>,
        mapped_buffer: Option<Buffer>,
        format: PixelFormat,
        size: Dimensions<i32>,
        duration: Option<Duration>,
    ) -> Self {
        Self {
            data: FrameData::Gpu(GpuFrameData::new(resource, mapped_buffer)),
            format,
            size,
            duration,
        }
    }

    pub fn gpu(&self) -> Option<&GpuFrameData> {
        match &self.data {
            FrameData::Software(_) => None,
            FrameData::Gpu(gpu) => Some(gpu),
        }
    }

    pub fn gpu_frame_id(&self) -> Option<GpuFrameId> {
        self.gpu().map(GpuFrameData::frame_id)
    }

    pub fn gpu_texture_id(&self) -> Option<GpuTextureId> {
        self.gpu().map(GpuFrameData::texture_id)
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn d3d11_texture(
        &self,
    ) -> Option<&windows::Win32::Graphics::Direct3D11::ID3D11Texture2D> {
        self.gpu().map(GpuFrameData::d3d11_texture)
    }

    /// Returns CPU pixels when the frame has a software-readable representation.
    pub fn software_pixels(&self) -> Option<bytes::Bytes> {
        match &self.data {
            FrameData::Software(buffer) => Some(buffer.clone()),
            FrameData::Gpu(gpu) => gpu.software_pixels(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra8_is_converted_and_relabelled_as_rgba8() {
        let mut pixels = [10, 20, 30, 255, 40, 50, 60, 255];
        let mut format = PixelFormat::BGRA8;

        ensure_rgba8(&mut pixels, &mut format);

        assert_eq!(format, PixelFormat::RGBA8);
        assert_eq!(pixels, [30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn unsupported_formats_are_not_relabelled_as_rgba8() {
        let mut pixels = [0; 8];
        let mut format = PixelFormat::NV12;

        ensure_rgba8(&mut pixels, &mut format);

        assert_eq!(format, PixelFormat::NV12);
    }
}

//! CPU- and GPU-backed video frame data.

use std::{hash::Hash, sync::Arc, time::Duration};

use crate::media::{Dimensions, PixelFormat, buffer_pool::Buffer};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuImportHandle {
    #[cfg(target_os = "windows")]
    WindowsNtHandle(windows::Win32::Foundation::HANDLE),
}

impl Hash for GpuImportHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);

        match self {
            #[cfg(target_os = "windows")]
            Self::WindowsNtHandle(handle) => handle.0.hash(state),
        }
    }
}

unsafe impl Send for GpuImportHandle {}
unsafe impl Sync for GpuImportHandle {}

impl GpuImportHandle {
    #[cfg(target_os = "windows")]
    pub const fn from_windows_nt_handle(handle: windows::Win32::Foundation::HANDLE) -> Self {
        Self::WindowsNtHandle(handle)
    }

    #[cfg(target_os = "windows")]
    pub const fn as_windows_nt_handle(self) -> Option<windows::Win32::Foundation::HANDLE> {
        match self {
            Self::WindowsNtHandle(handle) => Some(handle),
        }
    }
}

#[derive(Debug)]
pub enum GpuFrameResource {
    #[cfg(target_os = "windows")]
    D3D11Texture(windows::Win32::Graphics::Direct3D11::ID3D11Texture2D),
}

#[derive(Debug)]
pub struct GpuFrameData {
    resource: GpuFrameResource,
    import_handle: Option<GpuImportHandle>,
    mapped_buffer: Option<bytes::Bytes>,
    _resource_owner: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

impl GpuFrameData {
    pub(crate) fn new(
        resource: GpuFrameResource,
        import_handle: Option<GpuImportHandle>,
        mapped_buffer: Option<Buffer>,
        resource_owner: Option<Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Self {
        Self {
            resource,
            import_handle,
            mapped_buffer: mapped_buffer.map(Buffer::freeze),
            _resource_owner: resource_owner,
        }
    }

    pub const fn import_handle(&self) -> Option<GpuImportHandle> {
        self.import_handle
    }

    pub fn software_pixels(&self) -> Option<bytes::Bytes> {
        self.mapped_buffer.clone()
    }

    #[cfg(target_os = "windows")]
    pub fn d3d11_texture(&self) -> Option<&windows::Win32::Graphics::Direct3D11::ID3D11Texture2D> {
        match &self.resource {
            GpuFrameResource::D3D11Texture(texture) => Some(texture),
        }
    }
}

#[derive(Debug)]
pub enum FrameData {
    /// CPU-accessible memory buffer.
    Software(bytes::Bytes),

    /// GPU-backed frame data with an optional zero-copy import handle.
    Gpu(GpuFrameData),
}

#[derive(Debug)]
pub struct Frame {
    pub data: FrameData,
    pub format: PixelFormat,
    pub size: Dimensions<i32>,
    pub duration: Option<Duration>,
}

unsafe impl Send for Frame {}
unsafe impl Sync for Frame {}

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
        resource: GpuFrameResource,
        import_handle: Option<GpuImportHandle>,
        mapped_buffer: Option<Buffer>,
        resource_owner: Option<Arc<dyn std::any::Any + Send + Sync>>,
        format: PixelFormat,
        size: Dimensions<i32>,
        duration: Option<Duration>,
    ) -> Self {
        Self {
            data: FrameData::Gpu(GpuFrameData::new(
                resource,
                import_handle,
                mapped_buffer,
                resource_owner,
            )),
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

    pub fn gpu_import_handle(&self) -> Option<GpuImportHandle> {
        self.gpu().and_then(GpuFrameData::import_handle)
    }

    #[cfg(target_os = "windows")]
    pub fn d3d11_texture(&self) -> Option<&windows::Win32::Graphics::Direct3D11::ID3D11Texture2D> {
        self.gpu().and_then(GpuFrameData::d3d11_texture)
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

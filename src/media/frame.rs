use std::{hash::Hash, sync::Arc, time::Duration};

use crate::{
    media::pixel_format::PixelFormat,
    utils::{bitmap_utils::ensure_rgba8, buffer_pool::Buffer, vector2::Vector2},
};

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
    pub fn new(
        resource: GpuFrameResource,
        import_handle: Option<GpuImportHandle>,
        mapped_buffer: Option<Buffer>,
        resource_owner: Option<Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Self {
        Self {
            resource,
            import_handle,
            mapped_buffer: mapped_buffer.map(|b| b.freeze()),
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
    /// CPU-accessible memory buffer
    Software(bytes::Bytes),

    /// GPU-backed frame data with optional import handle for zero-copy preview.
    Gpu(GpuFrameData),
}

#[derive(Debug)]
pub struct Frame {
    pub data: FrameData,
    pub format: PixelFormat,
    pub size: Vector2<i32>,
    pub duration: Option<Duration>,
}

unsafe impl Send for Frame {}
unsafe impl Sync for Frame {}

impl Frame {
    pub fn new_software(
        mut data: Buffer,
        mut format: PixelFormat,
        size: Vector2<i32>,
        duration: Option<Duration>,
    ) -> Self {
        ensure_rgba8(&mut data, &mut format);
        Frame { data: FrameData::Software(data.freeze()), format, size, duration }
    }

    pub fn new_gpu(
        resource: GpuFrameResource,
        import_handle: Option<GpuImportHandle>,
        mapped_buffer: Option<Buffer>,
        resource_owner: Option<Arc<dyn std::any::Any + Send + Sync>>,
        format: PixelFormat,
        size: Vector2<i32>,
        duration: Option<Duration>,
    ) -> Self {
        Frame {
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
        self.gpu().and_then(|gpu| gpu.d3d11_texture())
    }

    /// Helper to get CPU pixels if available (clones the Bytes handle)
    pub fn get_software_pixels(&self) -> Option<bytes::Bytes> {
        match &self.data {
            FrameData::Software(buf) => Some(buf.clone()),
            FrameData::Gpu(gpu) => gpu.software_pixels(),
        }
    }
}

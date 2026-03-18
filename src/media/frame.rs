use std::time::Duration;

use crate::{
    media::pixel_format::PixelFormat,
    utils::{bitmap_utils::ensure_rgba8, buffer_pool::Buffer, vector2::Vector2},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncHandle(pub windows::Win32::Foundation::HANDLE);

impl std::hash::Hash for SyncHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.0.hash(state);
    }
}

unsafe impl Send for SyncHandle {}
unsafe impl Sync for SyncHandle {}

#[derive(Debug)]
pub enum FrameData {
    /// CPU-accessible memory buffer
    Software(bytes::Bytes),

    /// Windows Zero-Copy handle
    #[cfg(target_os = "windows")]
    D3D11 {
        texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        /// Optional shared handle for cross-API zero-copy (e.g. wgpu)
        shared_handle: Option<SyncHandle>,
        /// Optional cached CPU mapping for UI preview
        mapped_buffer: Option<bytes::Bytes>,
        /// Object to keep alive until this frame is dropped
        keep_alive: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
    },
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

    #[cfg(target_os = "windows")]
    pub fn new_d3d11(
        texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        shared_handle: Option<SyncHandle>,
        mapped_buffer: Option<Buffer>,
        keep_alive: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
        format: PixelFormat,
        size: Vector2<i32>,
        duration: Option<Duration>,
    ) -> Self {
        Frame {
            data: FrameData::D3D11 {
                texture,
                shared_handle,
                mapped_buffer: mapped_buffer.map(|b| b.freeze()),
                keep_alive,
            },
            format,
            size,
            duration,
        }
    }

    /// Helper to get CPU pixels if available (clones the Bytes handle)
    pub fn get_software_pixels(&self) -> Option<bytes::Bytes> {
        match &self.data {
            FrameData::Software(buf) => Some(buf.clone()),
            #[cfg(target_os = "windows")]
            FrameData::D3D11 { mapped_buffer, .. } => mapped_buffer.clone(),
        }
    }
}

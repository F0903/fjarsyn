//! GPU frame import, producer-readiness synchronization, and preview policy.

use crate::media::{PixelFormat, frame::Frame};

#[cfg(target_os = "windows")]
mod dx12;
mod import_error;

pub use import_error::ImportError;

/// A wgpu texture imported from one exact immutable engine frame resource.
///
/// The import retains the native producer resource and shared fence for at
/// least its own cached lifetime. Import also queues an internal readiness
/// marker that retains those native owners until the producer wait completes,
/// so a cloned wgpu view cannot outlive the synchronization it depends on.
pub struct ImportedFrameTexture {
    texture: wgpu::Texture,
    resource_id: crate::media::frame::GpuResourceId,
    #[cfg(target_os = "windows")]
    _source: std::sync::Arc<crate::media::frame::GpuResource>,
    #[cfg(target_os = "windows")]
    _ready_fence: windows::Win32::Graphics::Direct3D12::ID3D12Fence,
}

impl ImportedFrameTexture {
    /// Creates a sampling view for this immutable imported texture.
    ///
    /// The view owns the imported D3D12 resource. Producer-side ownership and
    /// readiness are independently retained until the queued wait completes.
    pub fn create_view(&self) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub const fn resource_id(&self) -> crate::media::frame::GpuResourceId {
        self.resource_id
    }
}

pub fn supports_zero_copy_preview(format: PixelFormat) -> bool {
    #[cfg(target_os = "windows")]
    {
        dx12::supports_zero_copy_preview(format)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = format;
        false
    }
}

pub fn requires_cpu_readback(
    preview_enabled: bool,
    format: PixelFormat,
    hardware_backed_encoder: bool,
) -> bool {
    !hardware_backed_encoder
        || (preview_enabled
            && !supports_zero_copy_preview(format)
            && format.supports_software_preview())
}

/// Imports `frame` and queues its producer-fence wait on the same D3D12 queue
/// that will later sample it.
///
/// Import failures are typed so a renderer can upload retained CPU pixels or
/// present an explicit degraded state instead of silently drawing nothing.
pub fn import_frame_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &Frame,
) -> Result<ImportedFrameTexture, ImportError> {
    #[cfg(target_os = "windows")]
    {
        dx12::import_frame_texture(device, queue, frame)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (device, queue, frame);
        Err(ImportError::UnsupportedBackend)
    }
}

#[cfg(test)]
mod tests;

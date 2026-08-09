//! GPU texture import, producer-readiness synchronization, and preview policy.

use crate::media::{PixelFormat, frame::Frame};

#[cfg(target_os = "windows")]
mod dx12;
mod import_error;

pub use import_error::ImportError;

/// A wgpu texture imported from one pooled engine texture allocation.
///
/// One import may be reused by several non-overlapping `GpuFrameId` values.
/// Every actual draw must call [`Self::prepare_draw`] so producer readiness and
/// consumer completion remain attached to that exact frame.
pub struct ImportedFrameTexture {
    texture: wgpu::Texture,
    texture_id: crate::media::frame::GpuTextureId,
    #[cfg(target_os = "windows")]
    ready_fence: windows::Win32::Graphics::Direct3D12::ID3D12Fence,
    #[cfg(target_os = "windows")]
    device: windows::Win32::Graphics::Direct3D12::ID3D12Device,
}

impl ImportedFrameTexture {
    /// Creates a sampling view for this imported pooled texture.
    ///
    /// The returned view may outlive this wrapper, so the type system cannot
    /// couple it to the publication lease on its own.
    ///
    /// # Safety
    ///
    /// Every command buffer that samples the returned view must first call
    /// [`Self::prepare_draw`] with the exact `Frame` publication and retain the
    /// returned guard until that command buffer has completed on the GPU. The
    /// view must not be sampled after its frame lease is released without
    /// preparing a newer publication backed by the same texture.
    pub unsafe fn create_view(&self) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub const fn texture_id(&self) -> crate::media::frame::GpuTextureId {
        self.texture_id
    }

    /// Queues this frame's producer-fence wait immediately before an actual
    /// draw and returns the opaque lease that must be retained until that
    /// draw's command buffer finishes on the GPU.
    pub fn prepare_draw(
        &self,
        queue: &wgpu::Queue,
        frame: &Frame,
    ) -> Result<ImportedFrameDrawGuard, ImportError> {
        #[cfg(target_os = "windows")]
        {
            dx12::prepare_draw(self, queue, frame)
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = (queue, frame);
            Err(ImportError::UnsupportedBackend)
        }
    }
}

/// Opaque ownership retained until one imported draw completes on the GPU.
#[must_use = "the draw guard must be retained until its submitted GPU work completes"]
pub struct ImportedFrameDrawGuard {
    #[cfg(target_os = "windows")]
    _source: std::sync::Arc<crate::media::frame::GpuResource>,
    #[cfg(target_os = "windows")]
    _ready_fence: windows::Win32::Graphics::Direct3D12::ID3D12Fence,
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

/// Imports the pooled texture allocation that backs `frame`.
///
/// Import failures are typed so a renderer can upload retained CPU pixels or
/// present an explicit degraded state instead of silently drawing nothing.
pub fn import_frame_texture(
    device: &wgpu::Device,
    frame: &Frame,
) -> Result<ImportedFrameTexture, ImportError> {
    #[cfg(target_os = "windows")]
    {
        dx12::import_frame_texture(device, frame)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (device, frame);
        Err(ImportError::UnsupportedBackend)
    }
}

#[cfg(test)]
mod tests;

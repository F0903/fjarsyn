//! GPU frame import and preview/readback capability decisions.

use crate::media::{PixelFormat, frame::Frame};

#[cfg(target_os = "windows")]
mod dx12;

pub struct ImportedFrameTexture {
    pub texture: wgpu::Texture,
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

pub fn import_frame_texture(device: &wgpu::Device, frame: &Frame) -> Option<ImportedFrameTexture> {
    #[cfg(target_os = "windows")]
    {
        dx12::import_frame_texture(device, frame)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (device, frame);
        None
    }
}

#[cfg(test)]
mod tests;

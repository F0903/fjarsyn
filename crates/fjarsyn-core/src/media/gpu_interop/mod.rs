use crate::media::{ffmpeg::HWAccelType, frame::Frame, pixel_format::PixelFormat};

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
    hw_accel: HWAccelType,
) -> bool {
    hw_accel == HWAccelType::None
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

#[cfg(target_os = "windows")]
mod dx12;

#[cfg(test)]
mod tests {
    use super::requires_cpu_readback;
    use crate::media::{ffmpeg::HWAccelType, pixel_format::PixelFormat};

    #[test]
    fn software_encoding_always_requires_readback() {
        assert!(requires_cpu_readback(false, PixelFormat::BGRA8, HWAccelType::None));
    }

    #[test]
    fn zero_copy_preview_with_hw_encoding_skips_readback() {
        assert!(!requires_cpu_readback(true, PixelFormat::BGRA8, HWAccelType::D3D11VA));
    }

    #[test]
    fn disabled_preview_with_hw_encoding_skips_readback() {
        assert!(!requires_cpu_readback(false, PixelFormat::BGRA8, HWAccelType::D3D11VA));
    }
}

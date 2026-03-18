use crate::media::{ffmpeg::HWAccelType, pixel_format::PixelFormat};

pub fn backend_supports_zero_copy_preview() -> bool {
    #[cfg(target_os = "windows")]
    {
        std::env::var("WGPU_BACKEND")
            .map(|backend| backend.eq_ignore_ascii_case("dx12"))
            .unwrap_or(true)
    }

    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn can_zero_copy_preview(format: PixelFormat) -> bool {
    backend_supports_zero_copy_preview() && format.supports_zero_copy_preview()
}

pub fn requires_cpu_readback(
    preview_enabled: bool,
    format: PixelFormat,
    hw_accel: HWAccelType,
) -> bool {
    hw_accel == HWAccelType::None
        || (preview_enabled && !can_zero_copy_preview(format) && format.supports_software_preview())
}

#[cfg(test)]
mod tests {
    use super::requires_cpu_readback;
    use crate::media::{ffmpeg::HWAccelType, pixel_format::PixelFormat};

    #[test]
    fn software_encoding_always_requires_readback() {
        assert!(requires_cpu_readback(false, PixelFormat::BGRA8, HWAccelType::None));
    }

    #[test]
    fn preview_requires_readback_when_zero_copy_backend_is_unavailable() {
        let original_backend = std::env::var("WGPU_BACKEND").ok();
        unsafe {
            std::env::set_var("WGPU_BACKEND", "vulkan");
        }

        let requires_readback =
            requires_cpu_readback(true, PixelFormat::BGRA8, HWAccelType::D3D11VA);

        if let Some(backend) = original_backend {
            unsafe {
                std::env::set_var("WGPU_BACKEND", backend);
            }
        } else {
            unsafe {
                std::env::remove_var("WGPU_BACKEND");
            }
        }

        assert!(requires_readback);
    }

    #[test]
    fn disabled_preview_with_hw_encoding_skips_readback() {
        assert!(!requires_cpu_readback(false, PixelFormat::BGRA8, HWAccelType::D3D11VA));
    }
}

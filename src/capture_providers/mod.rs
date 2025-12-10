mod capture_provider;
pub mod shared;
pub mod windows;

pub use capture_provider::CaptureProvider;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[cfg(target_os = "windows")]
    #[error(transparent)]
    WindowsCaptureError(#[from] windows::error::WindowsCaptureError),
}

#[cfg(target_os = "windows")]
pub const TARGET_PIXEL_FORMAT: PixelFormat = PixelFormat::RGBA8;

#[cfg(target_os = "windows")]
pub use windows::WgcCaptureProvider as PlatformCaptureProvider;
#[cfg(target_os = "windows")]
pub use windows::WindowsCaptureStream as PlatformCaptureStream;
#[cfg(target_os = "windows")]
pub use windows::user_pick_capture_item as user_pick_platform_capture_item;

#[cfg(target_os = "windows")]
use crate::capture_providers::shared::PixelFormat;

#[cfg(target_os = "windows")]
pub type PlatformCaptureItem = <PlatformCaptureProvider as CaptureProvider>::CaptureItem;

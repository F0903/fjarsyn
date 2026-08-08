//! Platform capture contracts and operating-system capture implementations.

mod platform;
mod provider;
#[cfg(target_os = "windows")]
mod windows;

pub use platform::{PlatformItem, pick_platform_item};
pub use provider::Provider;
#[cfg(target_os = "windows")]
pub use windows::{
    Builder as PlatformProviderBuilder, BuilderError as PlatformProviderBuilderError, Error,
    Provider as PlatformProvider, Stream as PlatformStream,
};

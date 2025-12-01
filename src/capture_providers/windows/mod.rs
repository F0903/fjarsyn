//mod builder;
//mod capture_provider;
mod capture_stream;
mod d3d11_utils;
pub(super) mod error;
mod wgc_capture_provider;
mod wgc_capture_provider_builder;

//pub use builder::{BuilderError, WindowsCaptureProviderBuilder};
//pub use capture_provider::WindowsCaptureProvider;
pub use capture_stream::WindowsCaptureStream;
pub use d3d11_utils::{create_capture_item_for_primary_monitor, user_pick_capture_item};
pub(self) use error::{Result, WindowsCaptureError};
pub use wgc_capture_provider::WgcCaptureProvider;
pub use wgc_capture_provider_builder::{WgcCaptureProviderBuilder, WgcCaptureProviderBuilderError};

mod capture_stream;
mod d3d11_utils;
pub(super) mod error;
mod wgc;

pub use capture_stream::WindowsCaptureStream;
pub use d3d11_utils::{create_capture_item_for_primary_monitor, user_pick_capture_item};
use error::{Result, WindowsCaptureError};
pub use wgc::{WgcCaptureProvider, WgcCaptureProviderBuilder, WgcCaptureProviderBuilderError};

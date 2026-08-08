//! Windows capture selection, WGC sessions, and D3D11 resource handling.

mod d3d11;
mod error;
pub(super) mod selection;
mod wgc;

pub(super) use d3d11::{
    copy_texture, create_d3d_device, map_read_texture, native_to_winrt_d3d11device,
    winrt_to_native_d3d11device,
};
pub use error::Error;
use error::Result;
pub use wgc::{Builder, BuilderError, Provider, Stream};

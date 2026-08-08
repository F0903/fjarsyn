//! D3D11 device conversion and CPU readback infrastructure.

mod device;
mod texture_readback;

pub(in crate::media::capture) use device::{
    create_d3d_device, native_to_winrt_d3d11device, winrt_to_native_d3d11device,
};
pub(in crate::media::capture) use texture_readback::{copy_texture, map_read_texture};

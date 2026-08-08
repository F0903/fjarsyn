//! Capture, codec execution, video frames, and GPU interoperability.

pub(crate) mod buffer_pool;
pub mod capture;
pub mod codec;
mod codec_device_lease;
mod dimensions;
pub mod frame;
pub mod gpu_interop;
mod pixel_format;
pub mod video;

pub use codec_device_lease::CodecDeviceLease;
pub use dimensions::Dimensions;
pub use pixel_format::PixelFormat;

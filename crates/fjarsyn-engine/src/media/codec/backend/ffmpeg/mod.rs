//! Private FFmpeg codec implementation for the supervised codec capability.

#[cfg(target_os = "windows")]
mod d3d11va_device_context;
mod decoder;
mod encoder;
mod transcode;

#[cfg(target_os = "windows")]
use d3d11va_device_context::D3d11vaDeviceContext;
pub(super) use decoder::Decoder;
pub(super) use encoder::Encoder;
pub(in crate::media::codec::backend) use transcode::{DecoderInfo, HardwareAcceleration};

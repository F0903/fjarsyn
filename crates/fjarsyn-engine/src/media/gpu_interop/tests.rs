use super::requires_cpu_readback;
use crate::media::PixelFormat;

#[test]
fn software_encoding_always_requires_readback() {
    assert!(requires_cpu_readback(false, PixelFormat::BGRA8, false));
}

#[test]
fn zero_copy_preview_with_hw_encoding_skips_readback() {
    assert!(!requires_cpu_readback(true, PixelFormat::BGRA8, true));
}

#[test]
fn disabled_preview_with_hw_encoding_skips_readback() {
    assert!(!requires_cpu_readback(false, PixelFormat::BGRA8, true));
}

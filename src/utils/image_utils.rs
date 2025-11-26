use crate::capture_providers::shared::PixelFormat;

pub fn ensure_image_rgba(bytes: &mut [u8], image_format: &mut PixelFormat) {
    match image_format {
        PixelFormat::RGBA8 => (),
        PixelFormat::BGRA8 => bgra_to_rgba(bytes),
    };
    *image_format = PixelFormat::RGBA8;
}

pub fn bgra_to_rgba(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2); // swap B and R
    }
}

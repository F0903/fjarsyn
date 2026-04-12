use crate::media::pixel_format::PixelFormat;

#[inline]
pub fn ensure_rgba8(bitmap: &mut [u8], src_format: &mut PixelFormat) {
    match src_format {
        PixelFormat::RGBA16 => (), // TODO: Support RGBA16 conversion
        PixelFormat::RGBA10 => (), // TODO: Support RGBA10 conversion
        PixelFormat::RGBA8 => (),
        PixelFormat::BGRA8 => bgra8_to_rgba8(bitmap),
        PixelFormat::NV12 => (), // TODO: Support NV12 conversion
    };
    *src_format = PixelFormat::RGBA8;
}

// TODO: SIMD
#[inline]
fn swap_first_channel(bitmap: &mut [u8]) {
    let len = bitmap.len();
    let mut ptr = bitmap.as_mut_ptr();
    // Calculate the end pointer aligned to the last complete pixel.
    // We ignore any trailing bytes that don't make up a full 4-byte pixel.
    let end = unsafe { ptr.add(len - (len % 4)) };

    unsafe {
        while ptr < end {
            // Swap the 1st byte and 3rd byte.
            // BGRA -> RGBA (for example)
            let first_byte = *ptr;
            let third_byte_ptr = ptr.add(2);
            *ptr = *third_byte_ptr;
            *third_byte_ptr = first_byte;

            // Advance to the next pixel (4 bytes)
            ptr = ptr.add(4);
        }
    }
}

/// Swaps the Blue and Red channels in a BGRA8 buffer to convert it to RGBA8.
///
/// # Safety
/// This function relies on `bytes` containing valid BGRA8/RGBA8 data (4 bytes per pixel).
/// It processes chunks of 4 bytes. If the buffer length is not a multiple of 4,
/// the trailing bytes are ignored (which is correct for pixel data).
#[inline]
pub fn bgra8_to_rgba8(bgra8: &mut [u8]) {
    swap_first_channel(bgra8);
}

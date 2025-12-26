use crate::utils::pixel_format::PixelFormat;

#[inline]
pub fn ensure_rgba(bitmap: &mut [u8], src_format: &mut PixelFormat) {
    match src_format {
        PixelFormat::RGBA16 => (),
        PixelFormat::RGBA8 => (),
        PixelFormat::BGRA8 => bgra8_to_rgba8(bitmap),
    };
    *src_format = PixelFormat::RGBA8;
}

//TODO: SIMD
#[inline]
fn swap_first_channel(bitmap: &mut [u8]) {
    let len = bitmap.len();
    let mut ptr = bitmap.as_mut_ptr();
    // Calculate the end pointer aligned to the last complete pixel.
    // We ignore any trailing bytes that don't make up a full 4-byte pixel.
    let end = unsafe { ptr.add(len - (len % 4)) };

    unsafe {
        while ptr < end {
            // Swap the 1st byte (Blue) and 3rd byte (Red).
            // BGRA -> RGBA
            let tmp = *ptr;
            *ptr = *ptr.add(2);
            *ptr.add(2) = tmp;

            // Advance to the next pixel (4 bytes)
            ptr = ptr.add(4);
        }
    }
}

/// Swaps the Blue and Red channels in a BGRA buffer to convert it to RGBA.
///
/// # Safety
/// This function relies on `bytes` containing valid BGRA/RGBA data (4 bytes per pixel).
/// It processes chunks of 4 bytes. If the buffer length is not a multiple of 4,
/// the trailing bytes are ignored (which is correct for pixel data).
#[inline]
pub fn bgra8_to_rgba8(bgra8: &mut [u8]) {
    swap_first_channel(bgra8);
}

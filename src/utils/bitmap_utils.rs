use crate::capture_providers::shared::PixelFormat;

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

/// Swaps the Red and Blue channels in a RGBA buffer to convert it to BGRA.
///
/// # Safety
/// This function relies on `bytes` containing valid BGRA/RGBA data (4 bytes per pixel).
/// It processes chunks of 4 bytes. If the buffer length is not a multiple of 4,
/// the trailing bytes are ignored (which is correct for pixel data).
#[inline]
#[allow(dead_code)]
pub fn rgba8_to_bgra8(rgba8: &mut [u8]) {
    swap_first_channel(rgba8);
}

//TODO: SIMD
#[inline]
pub fn rgba8_to_yuv420(
    rgba8: &[u8],
    width: usize,
    height: usize,
    stride_pixels: usize,
    yuv: &mut [u8],
) {
    let u_start = width * height;
    let v_start = u_start + u_start / 4;
    let mut y_idx = 0;
    let mut u_idx = u_start;
    let mut v_idx = v_start;

    for j in 0..height {
        for i in 0..width {
            let r_idx = (j * stride_pixels + i) * 4;
            let r = rgba8[r_idx] as i32;
            let g = rgba8[r_idx + 1] as i32;
            let b = rgba8[r_idx + 2] as i32;
            let _a = rgba8[r_idx + 3];

            // Integer conversion (ITU-R BT.601)
            let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            yuv[y_idx] = y.clamp(0, 255) as u8;
            y_idx += 1;

            if j % 2 == 0 && i % 2 == 0 {
                let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
                let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
                yuv[u_idx] = u.clamp(0, 255) as u8;
                yuv[v_idx] = v.clamp(0, 255) as u8;
                u_idx += 1;
                v_idx += 1;
            }
        }
    }
}

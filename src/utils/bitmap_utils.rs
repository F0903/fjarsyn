use crate::capture_providers::shared::PixelFormat;

pub fn ensure_rgba(bitmap: &mut [u8], src_format: &mut PixelFormat) {
    match src_format {
        PixelFormat::RGBA16 => (),
        PixelFormat::RGBA8 => (),
        PixelFormat::BGRA8 => bgra8_to_rgba8(bitmap),
    };
    *src_format = PixelFormat::RGBA8;
}

//TODO: SIMD
/// Swaps the Blue and Red channels in a BGRA buffer to convert it to RGRA/RGBA.
///
/// # Safety
/// This function relies on `bytes` containing valid BGRA/RGBA data (4 bytes per pixel).
/// It processes chunks of 4 bytes. If the buffer length is not a multiple of 4,
/// the trailing bytes are ignored (which is correct for pixel data).
pub fn bgra8_to_rgba8(bgra8: &mut [u8]) {
    let len = bgra8.len();
    let mut ptr = bgra8.as_mut_ptr();
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

//TODO: SIMD
/// Converts an RGB8 image to I420 YUV format.
pub fn rgb_to_yuv(rgb8: &[u8], width: usize, height: usize, yuv: &mut [u8]) {
    let u_start = width * height;
    let v_start = u_start + u_start / 4;
    let mut y_idx = 0;
    let mut u_idx = u_start;
    let mut v_idx = v_start;

    for j in 0..height {
        for i in 0..width {
            let r_idx = (j * width + i) * 3;
            let r = rgb8[r_idx] as f32;
            let g = rgb8[r_idx + 1] as f32;
            let b = rgb8[r_idx + 2] as f32;

            let y = 0.257 * r + 0.504 * g + 0.098 * b + 16.0;
            yuv[y_idx] = y.clamp(0.0, 255.0) as u8;
            y_idx += 1;

            if j % 2 == 0 && i % 2 == 0 {
                let u = -0.148 * r - 0.291 * g + 0.439 * b + 128.0;
                let v = 0.439 * r - 0.368 * g - 0.071 * b + 128.0;
                yuv[u_idx] = u.clamp(0.0, 255.0) as u8;
                yuv[v_idx] = v.clamp(0.0, 255.0) as u8;
                u_idx += 1;
                v_idx += 1;
            }
        }
    }
}

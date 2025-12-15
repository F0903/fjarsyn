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
pub fn rgba8_to_yuv420(rgba8: &[u8], width: usize, height: usize, yuv: &mut [u8]) {
    let u_start = width * height;
    let v_start = u_start + u_start / 4;
    let mut y_idx = 0;
    let mut u_idx = u_start;
    let mut v_idx = v_start;

    for j in 0..height {
        for i in 0..width {
            let r_idx = (j * width + i) * 4;
            let r = rgba8[r_idx] as f32;
            let g = rgba8[r_idx + 1] as f32;
            let b = rgba8[r_idx + 2] as f32;
            let _a = rgba8[r_idx + 3] as f32;

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

//TODO: SIMD
#[inline]
pub fn yuv420_to_rgba8(
    y_plane: &[u8],
    u_plane: &[u8],
    v_plane: &[u8],
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
    width: usize,
    height: usize,
    rgba8: &mut [u8],
) {
    for y in 0..height {
        for x in 0..width {
            let y_val = y_plane[y * y_stride + x] as f32;
            let u_val = u_plane[(y / 2) * u_stride + (x / 2)] as f32;
            let v_val = v_plane[(y / 2) * v_stride + (x / 2)] as f32;

            let c = y_val - 16.0;
            let d = u_val - 128.0;
            let e = v_val - 128.0;

            let r = (298.0 * c + 409.0 * e + 128.0) / 256.0;
            let g = (298.0 * c - 100.0 * d - 208.0 * e + 128.0) / 256.0;
            let b = (298.0 * c + 516.0 * d + 128.0) / 256.0;
            let a = 255.0_f32;

            let idx = (y * width + x) * 4;
            rgba8[idx] = r.clamp(0.0, 255.0) as u8;
            rgba8[idx + 1] = g.clamp(0.0, 255.0) as u8;
            rgba8[idx + 2] = b.clamp(0.0, 255.0) as u8;
            rgba8[idx + 3] = a.clamp(0.0, 255.0) as u8;
        }
    }
}

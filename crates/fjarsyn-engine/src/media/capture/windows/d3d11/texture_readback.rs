use std::mem::MaybeUninit;

use windows::Win32::Graphics::{
    Direct3D11::{D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, ID3D11DeviceContext, ID3D11Texture2D},
    Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_NV12},
};

use super::super::{Error, Result};

pub(in crate::media::capture) fn copy_texture(
    context: &ID3D11DeviceContext,
    source_texture: &ID3D11Texture2D,
    staging_texture: &ID3D11Texture2D,
) {
    unsafe {
        context.CopyResource(staging_texture, source_texture);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReadbackLayout {
    height: usize,
    bytes_per_row: usize,
    total_bytes: usize,
}

fn readback_layout(
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    bytes_per_pixel: u32,
) -> ReadbackLayout {
    debug_assert!(format == DXGI_FORMAT_NV12 || bytes_per_pixel > 0);

    let height = height as usize;
    let bytes_per_row = match format {
        DXGI_FORMAT_NV12 => width as usize,
        _ => width as usize * bytes_per_pixel as usize,
    };
    let total_bytes = match format {
        DXGI_FORMAT_NV12 => (width as usize * height * 3) / 2,
        _ => bytes_per_row * height,
    };

    ReadbackLayout { height, bytes_per_row, total_bytes }
}

pub(in crate::media::capture) fn map_read_texture(
    memory: &mut [u8],
    context: &ID3D11DeviceContext,
    staging_texture: &ID3D11Texture2D,
    texture_description: &D3D11_TEXTURE2D_DESC,
    bytes_per_pixel: u32,
) -> Result<()> {
    let start = std::time::Instant::now();
    let layout = readback_layout(
        texture_description.Width,
        texture_description.Height,
        texture_description.Format,
        bytes_per_pixel,
    );

    if memory.len() < layout.total_bytes {
        return Err(Error::ReadbackBufferTooSmall {
            expected: layout.total_bytes,
            actual: memory.len(),
        });
    }

    unsafe {
        let map_start = std::time::Instant::now();
        let mut mapped = MaybeUninit::uninit();
        context
            .Map(staging_texture, 0, D3D11_MAP_READ, 0, Some(mapped.as_mut_ptr()))
            .map_err(Error::FailedToMapTexture)?;
        let mapped = mapped.assume_init_ref();
        let map_duration = map_start.elapsed();
        let row_pitch = mapped.RowPitch as usize;

        let copy_start = std::time::Instant::now();
        if row_pitch == layout.bytes_per_row && texture_description.Format != DXGI_FORMAT_NV12 {
            std::ptr::copy_nonoverlapping(
                mapped.pData.cast(),
                memory.as_mut_ptr(),
                layout.total_bytes,
            );
        } else if texture_description.Format == DXGI_FORMAT_NV12 {
            for row in 0..layout.height {
                let source = mapped.pData.add(row * row_pitch);
                let destination = memory.as_mut_ptr().add(row * layout.bytes_per_row);
                std::ptr::copy_nonoverlapping(source.cast(), destination, layout.bytes_per_row);
            }
            let uv_source = mapped.pData.add(layout.height * row_pitch);
            let uv_destination = memory.as_mut_ptr().add(layout.height * layout.bytes_per_row);
            for row in 0..layout.height / 2 {
                let source = uv_source.add(row * row_pitch);
                let destination = uv_destination.add(row * layout.bytes_per_row);
                std::ptr::copy_nonoverlapping(source.cast(), destination, layout.bytes_per_row);
            }
        } else {
            for row in 0..layout.height {
                let source = mapped.pData.add(row * row_pitch);
                let destination = memory.as_mut_ptr().add(row * layout.bytes_per_row);
                std::ptr::copy_nonoverlapping(source.cast(), destination, layout.bytes_per_row);
            }
        }
        let copy_duration = copy_start.elapsed();

        context.Unmap(staging_texture, 0);
        let total = start.elapsed();
        if total.as_millis() > 2 {
            tracing::trace!(
                "map_read_texture perf: Total={:?}, Map={:?}, MemCpy={:?}",
                total,
                map_duration,
                copy_duration
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8B8A8_UNORM};

    use super::{ReadbackLayout, readback_layout};

    #[test]
    fn readback_layout_uses_packed_format_stride() {
        assert_eq!(
            readback_layout(1920, 1080, DXGI_FORMAT_R8G8B8A8_UNORM, 4),
            ReadbackLayout { height: 1080, bytes_per_row: 7680, total_bytes: 8_294_400 }
        );
    }

    #[test]
    fn readback_layout_uses_nv12_plane_size() {
        assert_eq!(
            readback_layout(1920, 1080, DXGI_FORMAT_NV12, 2),
            ReadbackLayout { height: 1080, bytes_per_row: 1920, total_bytes: 3_110_400 }
        );
    }
}

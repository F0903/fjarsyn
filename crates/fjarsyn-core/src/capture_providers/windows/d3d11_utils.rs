use std::{future::Future, mem::MaybeUninit, pin::Pin};

use windows::{
    Graphics::{
        Capture::{GraphicsCaptureItem, GraphicsCapturePicker},
        DirectX::Direct3D11::IDirect3DDevice,
    },
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0,
                D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
            },
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_SDK_VERSION,
                D3D11_TEXTURE2D_DESC, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Texture2D,
            },
            Dxgi::{Common::DXGI_FORMAT, IDXGIDevice},
            Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromWindow},
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
        UI::Shell::IInitializeWithWindow,
    },
};
use windows_core::*;

pub(super) fn create_d3d_device() -> Result<ID3D11Device> {
    tracing::debug!("Creating D3D11 device...");
    const FEATURE_LEVELS: &[D3D_FEATURE_LEVEL] = &[
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];

    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    let mut chosen_level = D3D_FEATURE_LEVEL_11_1;

    unsafe {
        D3D11CreateDevice(
            None, // adapter
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE(std::ptr::null_mut()),    // no software rasterizer
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, // flags
            Some(FEATURE_LEVELS),             // feature levels
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut chosen_level),
            Some(&mut context),
        )?;
    }

    let device = device.expect("ID3D11Device");
    let dxgi_device: IDXGIDevice = device.cast()?;

    // Make the immediate context thread-safe
    if let Ok(multithread) =
        device.cast::<windows::Win32::Graphics::Direct3D11::ID3D11Multithread>()
    {
        unsafe {
            let _ = multithread.SetMultithreadProtected(true);
        }
        tracing::info!("Enabled D3D11 multithread protection.");
    } else {
        tracing::warn!("Failed to get ID3D11Multithread, context may not be thread-safe!");
    }

    let adapter = unsafe { dxgi_device.GetAdapter()? };

    let desc = unsafe { adapter.GetDesc()? };
    let description = String::from_utf16_lossy(&desc.Description);
    let description = description.trim_matches(char::from(0));

    tracing::info!(
        "D3D11 device created successfully on adapter: '{}' with feature level: {:?}",
        description,
        chosen_level
    );

    Ok(device)
}

pub(super) fn native_to_winrt_d3d11device(device: &ID3D11Device) -> Result<IDirect3DDevice> {
    tracing::trace!("Converting native D3D11 device to WinRT D3D11 device");
    let dxgi_device: IDXGIDevice = device.cast()?;
    unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)?.cast() }
}

#[allow(dead_code)]
pub(super) fn winrt_to_native_d3d11device(device: &IDirect3DDevice) -> Result<ID3D11Device> {
    tracing::trace!("Converting WinRT D3D11 device to native D3D11 device");
    let access: IDirect3DDxgiInterfaceAccess = device.cast()?;
    unsafe {
        let raw = access.GetInterface::<ID3D11Device>()?;
        Ok(raw)
    }
}

pub trait IntoHWND {
    fn into_hwnd(self) -> HWND;
}

impl IntoHWND for HWND {
    fn into_hwnd(self) -> HWND {
        self
    }
}

impl IntoHWND for u64 {
    fn into_hwnd(self) -> HWND {
        HWND(self as usize as *mut core::ffi::c_void)
    }
}

/// Shows a dialog in the specified window to pick an item to capture.
/// Returned future completes when the user picks an item or cancels the dialog.
pub type PickCaptureItemFuture =
    Pin<Box<dyn Future<Output = Result<Option<GraphicsCaptureItem>>> + Send>>;

pub fn user_pick_capture_item(window: impl IntoHWND) -> Result<PickCaptureItemFuture> {
    tracing::info!("Initializing GraphicsCapturePicker...");
    let picker = GraphicsCapturePicker::new()?;
    let init_with_window: IInitializeWithWindow = picker.cast()?;
    unsafe { init_with_window.Initialize(window.into_hwnd())? };

    tracing::info!("Waiting for user to pick capture item...");
    let item_future = async move {
        let op = picker.PickSingleItemAsync()?;

        // Manual polling loop to wait for completion without relying on unstable Future impls
        while op.Status()? == windows::Foundation::AsyncStatus::Started {
            tokio::task::yield_now().await;
        }

        let result = op.GetResults();
        match &result {
            Ok(item) => tracing::info!(
                "User picked capture item: {:?}",
                item.DisplayName().unwrap_or_default()
            ),
            Err(e) => {
                // HRESULT(0) indicates the user cancelled the dialog
                if e.code() == HRESULT(0) {
                    return Ok(None);
                }
                tracing::error!("Error picking capture item: {:?}", e)
            }
        }
        result.map(Some)
    };
    Ok(Box::pin(item_future))
}

pub fn create_capture_item_for_primary_monitor() -> Result<GraphicsCaptureItem> {
    tracing::info!("Creating capture item for primary monitor...");
    let monitor_handle =
        unsafe { MonitorFromWindow(HWND(std::ptr::null_mut()), MONITOR_DEFAULTTOPRIMARY) };

    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    unsafe { interop.CreateForMonitor(monitor_handle) }
}

// Copy the texture from source to staging texture.
// This operation happens on the GPU.
pub(super) fn copy_texture(
    context: &ID3D11DeviceContext,
    source_tex: &ID3D11Texture2D,
    staging_tex: &ID3D11Texture2D,
) {
    unsafe {
        context.CopyResource(staging_tex, source_tex);
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
    debug_assert!(
        format == windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12 || bytes_per_pixel > 0
    );

    let height = height as usize;
    let bytes_per_row = match format {
        windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12 => width as usize,
        _ => width as usize * bytes_per_pixel as usize,
    };
    let total_bytes = match format {
        windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12 => {
            (width as usize * height * 3) / 2
        }
        _ => bytes_per_row * height,
    };

    ReadbackLayout { height, bytes_per_row, total_bytes }
}

// Map the staging texture for reading and copy the data to memory.
// This operation happens on the CPU.
pub(super) fn map_read_texture(
    memory: &mut [u8],
    context: &ID3D11DeviceContext,
    staging_tex: &ID3D11Texture2D,
    tex_desc: &D3D11_TEXTURE2D_DESC,
    bytes_per_pixel: u32,
) -> super::Result<()> {
    let start = std::time::Instant::now();
    let layout = readback_layout(tex_desc.Width, tex_desc.Height, tex_desc.Format, bytes_per_pixel);

    if memory.len() < layout.total_bytes {
        return Err(super::WindowsCaptureError::ReadbackBufferTooSmall {
            expected: layout.total_bytes,
            actual: memory.len(),
        });
    }

    unsafe {
        let map_start = std::time::Instant::now();
        let mut mapped = MaybeUninit::uninit();
        context
            .Map(staging_tex, 0, D3D11_MAP_READ, 0, Some(mapped.as_mut_ptr()))
            .map_err(super::WindowsCaptureError::FailedToMapTexture)?;
        let mapped = mapped.assume_init_ref();
        let map_duration = map_start.elapsed();

        let row_pitch = mapped.RowPitch as usize;

        let copy_start = std::time::Instant::now();
        if row_pitch == layout.bytes_per_row
            && tex_desc.Format != windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12
        {
            // If the pitch matches the width and it's a simple packed format, we can copy the entire buffer in one go.
            std::ptr::copy_nonoverlapping(
                mapped.pData.cast(),
                memory.as_mut_ptr(),
                layout.total_bytes,
            );
        } else if tex_desc.Format == windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12 {
            // NV12 Copy
            // Y Plane
            for y in 0..layout.height {
                let src_row = mapped.pData.add(y * row_pitch);
                let dst_row = memory.as_mut_ptr().add(y * layout.bytes_per_row);
                std::ptr::copy_nonoverlapping(src_row.cast(), dst_row, layout.bytes_per_row);
            }
            // UV Plane
            let uv_src_base = mapped.pData.add(layout.height * row_pitch);
            let uv_dst_base = memory.as_mut_ptr().add(layout.height * layout.bytes_per_row);
            for y in 0..layout.height / 2 {
                let src_row = uv_src_base.add(y * row_pitch);
                let dst_row = uv_dst_base.add(y * layout.bytes_per_row);
                std::ptr::copy_nonoverlapping(src_row.cast(), dst_row, layout.bytes_per_row);
            }
        } else {
            // Strided copy (handling padding bytes) for packed formats
            for y in 0..layout.height {
                let src_row = mapped.pData.add(y * row_pitch);
                let dst_row = memory.as_mut_ptr().add(y * layout.bytes_per_row);
                std::ptr::copy_nonoverlapping(src_row.cast(), dst_row, layout.bytes_per_row);
            }
        }
        let copy_duration = copy_start.elapsed();

        context.Unmap(staging_tex, 0);

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

// Copies the source texture to the staging texture on the GPU. Then maps the staging texture for reading, and copies its contents into the provided memory buffer.
#[allow(dead_code)]
pub(super) fn fetch_texture(
    dest: &mut [u8],
    context: &ID3D11DeviceContext,
    source_tex: ID3D11Texture2D,
    staging_tex: ID3D11Texture2D,
    tex_desc: &D3D11_TEXTURE2D_DESC,
    bytes_per_pixel: u32,
) -> super::Result<()> {
    copy_texture(context, &source_tex, &staging_tex);
    map_read_texture(dest, context, &staging_tex, tex_desc, bytes_per_pixel)
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8B8A8_UNORM};

    use super::*;

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

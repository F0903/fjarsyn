use std::mem::MaybeUninit;

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
            Dxgi::IDXGIDevice,
        },
        System::WinRT::Direct3D11::{
            CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
        },
        UI::Shell::IInitializeWithWindow,
    },
};
use windows_core::*;

pub(super) fn create_d3d_device() -> Result<ID3D11Device> {
    // We’ll request hardware device with default feature levels.
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

    Ok(device.expect("ID3D11Device"))
}

pub(super) fn native_to_winrt_d3d11device(device: &ID3D11Device) -> Result<IDirect3DDevice> {
    let dxgi_device: IDXGIDevice = device.cast()?;
    unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)?.cast() }
}

pub(super) fn winrt_to_native_d3d11device(device: &IDirect3DDevice) -> Result<ID3D11Device> {
    // WinRT device implements IDirect3DDxgiInterfaceAccess, which lets you retrieve
    // the underlying DXGI/D3D interfaces.
    let access: IDirect3DDxgiInterfaceAccess = device.cast()?;

    unsafe {
        // Request the ID3D11Device interface (identified by its IID).
        let raw = access.GetInterface::<ID3D11Device>()?;
        Ok(raw)
    }
}

pub(crate) trait IntoHWND {
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
pub(crate) fn user_pick_capture_item(
    window: impl IntoHWND,
) -> Result<impl Future<Output = Result<GraphicsCaptureItem>>> {
    let picker = GraphicsCapturePicker::new()?;
    let init_with_window: IInitializeWithWindow = picker.cast()?;
    unsafe { init_with_window.Initialize(window.into_hwnd())? };
    let item_future = async move { picker.PickSingleItemAsync()?.await };
    Ok(item_future)
}

pub(super) fn read_texture(
    context: &ID3D11DeviceContext,
    source_tex: ID3D11Texture2D,
    staging_tex: ID3D11Texture2D,
    tex_desc: &D3D11_TEXTURE2D_DESC,
    bytes_per_pixel: u32,
) -> std::result::Result<Vec<u8>, crate::CaptureError> {
    unsafe {
        context.CopyResource(&staging_tex, &source_tex);

        let mut mapped = MaybeUninit::uninit();
        context.Map(&staging_tex, 0, D3D11_MAP_READ, 0, Some(mapped.as_mut_ptr()))?;
        let mapped = mapped.assume_init_ref();

        let height = tex_desc.Height as usize;
        let bytes_per_row = tex_desc.Width as usize * bytes_per_pixel as usize;
        let row_pitch = mapped.RowPitch as usize;
        let total_bytes = bytes_per_row * height;

        let mut frame_bytes = vec![0u8; total_bytes];
        for y in 0..height {
            let src_row = mapped.pData.add(y * row_pitch);
            let dst_row_start = y * bytes_per_row;
            let dst_row_end = (y + 1) * bytes_per_row;
            let dst_row = &mut frame_bytes[dst_row_start..dst_row_end];
            std::ptr::copy_nonoverlapping(src_row.cast(), dst_row.as_mut_ptr(), bytes_per_row);
        }
        context.Unmap(&staging_tex, 0);

        Ok(frame_bytes)
    }
}

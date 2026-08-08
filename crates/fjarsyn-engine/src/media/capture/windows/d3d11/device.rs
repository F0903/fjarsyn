use windows::{
    Graphics::DirectX::Direct3D11::IDirect3DDevice,
    Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0,
                D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
            },
            Direct3D11::{
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
                ID3D11Device, ID3D11DeviceContext,
            },
            Dxgi::IDXGIDevice,
        },
        System::WinRT::Direct3D11::{
            CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
        },
    },
    core::{Interface, Result},
};

pub(in crate::media::capture) fn create_d3d_device() -> Result<ID3D11Device> {
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
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE(std::ptr::null_mut()),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(FEATURE_LEVELS),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut chosen_level),
            Some(&mut context),
        )?;
    }

    let device = device.expect("ID3D11Device");
    let dxgi_device: IDXGIDevice = device.cast()?;

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
    let adapter_description = unsafe { adapter.GetDesc()? };
    let description = String::from_utf16_lossy(&adapter_description.Description);
    let description = description.trim_matches(char::from(0));
    tracing::info!(
        "D3D11 device created successfully on adapter: '{}' with feature level: {:?}",
        description,
        chosen_level
    );

    Ok(device)
}

pub(in crate::media::capture) fn native_to_winrt_d3d11device(
    device: &ID3D11Device,
) -> Result<IDirect3DDevice> {
    tracing::trace!("Converting native D3D11 device to WinRT D3D11 device");
    let dxgi_device: IDXGIDevice = device.cast()?;
    unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)?.cast() }
}

pub(in crate::media::capture) fn winrt_to_native_d3d11device(
    device: &IDirect3DDevice,
) -> Result<ID3D11Device> {
    tracing::trace!("Converting WinRT D3D11 device to native D3D11 device");
    let access: IDirect3DDxgiInterfaceAccess = device.cast()?;
    unsafe { access.GetInterface::<ID3D11Device>() }
}

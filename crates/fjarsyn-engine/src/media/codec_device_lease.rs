#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

/// Owned graphics-device lease used by a hardware codec worker.
///
/// The Windows capture backend enables D3D11 multithread protection before
/// constructing this value. Keeping the COM interface owned avoids passing an
/// untracked raw device pointer to a codec thread.
#[derive(Debug, Clone)]
pub struct CodecDeviceLease {
    #[cfg(target_os = "windows")]
    device: ID3D11Device,
}

#[cfg(target_os = "windows")]
impl CodecDeviceLease {
    pub(crate) const fn from_d3d11(device: ID3D11Device) -> Self {
        Self { device }
    }

    pub(crate) const fn d3d11(&self) -> &ID3D11Device {
        &self.device
    }
}

// D3D11 devices are free-threaded once ID3D11Multithread protection is
// enabled. Construction is crate-private so only the capture backend that
// establishes that invariant can produce a lease.
#[cfg(target_os = "windows")]
unsafe impl Send for CodecDeviceLease {}
#[cfg(target_os = "windows")]
unsafe impl Sync for CodecDeviceLease {}

use windows::Graphics::{Capture::GraphicsCaptureItem, DirectX::Direct3D11::IDirect3DDevice};

use crate::capture_providers::{
    CaptureError,
    windows::{
        capture_provider::WindowsCaptureProvider,
        d3d11_utils::{create_d3d_device, native_to_winrt_d3d11device},
    },
};

type Result<T> = std::result::Result<T, BuilderError>;

#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
    #[error("Missing device")]
    MissingDevice,
    #[error("Initialization error: {0}")]
    InitializationError(#[from] CaptureError),
    #[error("Windows error: {0}")]
    WindowsError(#[from] windows::core::Error),
}

pub struct WindowsCaptureProviderBuilder {
    device: Option<IDirect3DDevice>,
    capture_item: Option<GraphicsCaptureItem>,
}

impl WindowsCaptureProviderBuilder {
    pub fn new() -> Self {
        WindowsCaptureProviderBuilder { device: None, capture_item: None }
    }

    #[allow(dead_code)]
    pub fn with_device(mut self, device: IDirect3DDevice) -> Self {
        self.device = Some(device);
        self
    }

    pub fn with_default_device(mut self) -> Result<Self> {
        let d3d_device = create_d3d_device()?;
        let winrt_device = native_to_winrt_d3d11device(&d3d_device)?;
        self.device = Some(winrt_device);
        Ok(self)
    }

    pub fn with_default_capture_item(mut self) -> Result<Self> {
        self.capture_item = None;
        Ok(self)
    }

    /// Must be called from the main thread.
    pub fn build(self) -> Result<WindowsCaptureProvider> {
        let device = self.device.ok_or(BuilderError::MissingDevice)?;
        WindowsCaptureProvider::new(device, self.capture_item).map_err(Into::into)
    }
}

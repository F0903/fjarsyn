use windows::Graphics::{Capture::GraphicsCaptureItem, DirectX::Direct3D11::IDirect3DDevice};

use crate::{
    capture_providers::{
        CaptureProvider,
        windows::{
            WgcCaptureProvider, WindowsCaptureError,
            d3d11_utils::{create_d3d_device, native_to_winrt_d3d11device},
        },
    },
    utils::pixel_format::PixelFormat,
};

type Result<T> = std::result::Result<T, WgcCaptureProviderBuilderError>;

#[derive(Debug, thiserror::Error)]
pub enum WgcCaptureProviderBuilderError {
    #[error("Missing device")]
    MissingDevice,
    #[error("Initialization error: {0}")]
    InitializationError(#[from] WindowsCaptureError),
    #[error("Windows error: {0}")]
    WindowsError(#[from] windows::core::Error),
}

pub struct WgcCaptureProviderBuilder {
    device: Option<IDirect3DDevice>,
    capture_item: Option<GraphicsCaptureItem>,
    pixel_format: PixelFormat,
}

impl WgcCaptureProviderBuilder {
    pub fn new(pixel_format: PixelFormat) -> Self {
        WgcCaptureProviderBuilder { device: None, capture_item: None, pixel_format }
    }

    #[allow(dead_code)]
    pub fn with_device(mut self, device: IDirect3DDevice) -> Self {
        tracing::debug!("Setting custom device for WindowsCaptureProviderBuilder");
        self.device = Some(device);
        self
    }

    pub fn with_default_device(mut self) -> Result<Self> {
        tracing::debug!("Initializing default capture device for WindowsCaptureProviderBuilder");
        let d3d_device = create_d3d_device()?;
        let winrt_device = native_to_winrt_d3d11device(&d3d_device)?;
        self.device = Some(winrt_device);
        Ok(self)
    }

    pub fn with_default_capture_item(mut self) -> Result<Self> {
        tracing::debug!("Using default capture item configuration");
        self.capture_item = None;
        Ok(self)
    }

    /// Must be called from the main thread.
    pub fn build(self) -> Result<WgcCaptureProvider> {
        tracing::info!("Building WindowsCaptureProvider");
        let device = self.device.ok_or_else(|| {
            tracing::error!("Attempted to build WindowsCaptureProvider without a device");
            WgcCaptureProviderBuilderError::MissingDevice
        })?;

        let mut capture = WgcCaptureProvider::new(device, self.pixel_format);
        if let Some(capture_item) = self.capture_item {
            capture.set_capture_item(capture_item)?;
        }
        Ok(capture)
    }
}

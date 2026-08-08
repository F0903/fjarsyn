use windows::Graphics::{Capture::GraphicsCaptureItem, DirectX::Direct3D11::IDirect3DDevice};

use super::{
    super::{Error, create_d3d_device, native_to_winrt_d3d11device},
    Provider,
};
use crate::media::{PixelFormat, capture::Provider as ProviderContract};

type Result<T> = std::result::Result<T, BuilderError>;

#[derive(Debug, thiserror::Error)]
pub enum BuilderError {
    #[error("Missing device")]
    MissingDevice,
    #[error("Initialization error: {0}")]
    InitializationError(#[from] Error),
    #[error("Windows error: {0}")]
    WindowsError(#[from] windows::core::Error),
}

pub struct Builder {
    device: Option<IDirect3DDevice>,
    capture_item: Option<GraphicsCaptureItem>,
    pixel_format: PixelFormat,
    record_cursor: bool,
    border_indicator: bool,
    cpu_readback_enabled: bool,
}

impl Builder {
    pub fn new(
        pixel_format: PixelFormat,
        record_cursor: bool,
        border_indicator: bool,
        cpu_readback_enabled: bool,
    ) -> Self {
        Self {
            device: None,
            capture_item: None,
            pixel_format,
            record_cursor,
            border_indicator,
            cpu_readback_enabled,
        }
    }

    pub fn with_default_device(mut self) -> Result<Self> {
        tracing::debug!("initializing the default WGC capture device");
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

    /// Builds the provider without a UI-thread dependency.
    ///
    /// Capture-source selection remains a desktop concern, while the selected
    /// `GraphicsCaptureItem` and the free-threaded capture pipeline are agile
    /// Windows Runtime objects that may cross thread boundaries.
    pub fn build(self) -> Result<Provider> {
        tracing::info!("building the WGC capture provider");
        let device = self.device.ok_or_else(|| {
            tracing::error!("attempted to build the WGC capture provider without a device");
            BuilderError::MissingDevice
        })?;

        let mut capture = Provider::new(
            device,
            self.pixel_format,
            self.record_cursor,
            self.border_indicator,
            self.cpu_readback_enabled,
        );
        if let Some(capture_item) = self.capture_item {
            capture.set_capture_item(capture_item)?;
        }
        Ok(capture)
    }
}

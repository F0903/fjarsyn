use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED,
    DXGI_ERROR_DEVICE_RESET, DXGI_ERROR_DRIVER_INTERNAL_ERROR,
};
use windows_core::HRESULT;

pub type Result<T> = std::result::Result<T, WindowsCaptureError>;

#[derive(Debug, thiserror::Error)]
pub enum WindowsCaptureError {
    #[error("Already capturing")]
    AlreadyCapturing,
    #[error("Not capturing")]
    NotCapturing,
    #[error("No frame pool available")]
    NoFramePool,
    #[error("No capture item available")]
    NoCaptureItem,
    #[error("Failed to lock staging state")]
    StagingStateLockFailed,
    #[error("Failed to create frame pool: {0}")]
    FailedToCreateFramePool(windows_core::Error),
    #[error("Failed to create capture session: {0}")]
    FailedToCreateCaptureSession(windows_core::Error),
    #[error("Failed to get capture item size: {0}")]
    FailedToGetCaptureItemSize(windows_core::Error),
    #[error("Failed to set min update interval: {0}")]
    FailedToSetMinUpdateInterval(windows_core::Error),
    #[error("Failed to set frame arrived handler: {0}")]
    FailedToSetFrameArrivedHandler(windows_core::Error),
    #[error("Failed to get surface: {0}")]
    FailedToGetSurface(windows_core::Error),
    #[error("Failed to get interface: {0}")]
    FailedToGetInterface(windows_core::Error),
    #[error("Failed to get content size: {0}")]
    FailedToGetContentSize(windows_core::Error),
    #[error("Failed to get device: {0}")]
    FailedToGetDevice(windows_core::Error),
    #[error("Failed to get immediate context: {0}")]
    FailedToGetImmediateContext(windows_core::Error),
    #[error("Failed to duplicate output: {0}")]
    DuplicateOutputFailed(windows_core::Error),
    #[error("Failed to create texture: {0}")]
    FailedToCreateTexture(windows_core::Error),
    #[error("Failed to recreate D3D device: {0}")]
    FailedToRecreateDevice(windows_core::Error),
    #[error("Failed to recreate WGC frame pool: {0}")]
    FailedToRecreateFramePool(windows_core::Error),
    #[error("Failed to recreate WGC capture session: {0}")]
    FailedToRecreateCaptureSession(windows_core::Error),
    #[error("Failed to map texture: {0}")]
    FailedToMapTexture(windows_core::Error),
    #[error("Readback buffer too small: expected at least {expected} bytes, got {actual}")]
    ReadbackBufferTooSmall { expected: usize, actual: usize },
    #[error("Failed to get monitor output: {0}")]
    FailedToGetMonitorOutput(windows_core::Error),
    #[error("Failed to create DispatcherQueueController: {0}")]
    FailedToCreateDispatcherQueueController(windows_core::Error),
    #[error("Failed to start capture: {0}")]
    FailedToStartCapture(windows_core::Error),
    #[error("Failed to start recreated capture session: {0}")]
    FailedToStartRecreatedCaptureSession(windows_core::Error),
    #[error("Failed to process frame")]
    FailedToProcessFrame(Box<WindowsCaptureError>),
    #[error("Windows smart pointer cast failed: {0}")]
    CastFailed(windows_core::Error),
    #[error("Invalid staging depth, staging depth can't be less than 1")]
    InvalidStagingDepth,
    #[error("Frame sender closed")]
    FrameSenderClosed,
    #[error("Unknown Windows error: {0}")]
    UnknownWindowsError(#[from] windows_core::Error),
}

impl WindowsCaptureError {
    pub(crate) fn is_recoverable_device_loss(&self) -> bool {
        match self {
            Self::FailedToProcessFrame(err) => err.is_recoverable_device_loss(),
            _ => self.windows_error().is_some_and(Self::is_recoverable_device_loss_error),
        }
    }

    pub(crate) fn is_recoverable_device_loss_error(error: &windows_core::Error) -> bool {
        Self::is_recoverable_device_loss_code(error.code())
    }

    pub(crate) fn is_recoverable_device_loss_code(code: HRESULT) -> bool {
        code == DXGI_ERROR_ACCESS_LOST
            || code == DXGI_ERROR_DEVICE_HUNG
            || code == DXGI_ERROR_DEVICE_REMOVED
            || code == DXGI_ERROR_DEVICE_RESET
            || code == DXGI_ERROR_DRIVER_INTERNAL_ERROR
    }

    fn windows_error(&self) -> Option<&windows_core::Error> {
        match self {
            Self::FailedToCreateFramePool(err)
            | Self::FailedToCreateCaptureSession(err)
            | Self::FailedToGetCaptureItemSize(err)
            | Self::FailedToSetMinUpdateInterval(err)
            | Self::FailedToSetFrameArrivedHandler(err)
            | Self::FailedToGetSurface(err)
            | Self::FailedToGetInterface(err)
            | Self::FailedToGetContentSize(err)
            | Self::FailedToGetDevice(err)
            | Self::FailedToGetImmediateContext(err)
            | Self::DuplicateOutputFailed(err)
            | Self::FailedToCreateTexture(err)
            | Self::FailedToRecreateDevice(err)
            | Self::FailedToRecreateFramePool(err)
            | Self::FailedToRecreateCaptureSession(err)
            | Self::FailedToMapTexture(err)
            | Self::FailedToGetMonitorOutput(err)
            | Self::FailedToCreateDispatcherQueueController(err)
            | Self::FailedToStartCapture(err)
            | Self::FailedToStartRecreatedCaptureSession(err)
            | Self::CastFailed(err)
            | Self::UnknownWindowsError(err) => Some(err),
            Self::AlreadyCapturing
            | Self::NotCapturing
            | Self::NoFramePool
            | Self::NoCaptureItem
            | Self::StagingStateLockFailed
            | Self::ReadbackBufferTooSmall { .. }
            | Self::FailedToProcessFrame(_)
            | Self::InvalidStagingDepth
            | Self::FrameSenderClosed => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Dxgi::{
        DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
    };

    use super::*;

    #[test]
    fn device_loss_hresult_codes_are_recoverable() {
        assert!(WindowsCaptureError::is_recoverable_device_loss_code(DXGI_ERROR_DEVICE_REMOVED));
        assert!(WindowsCaptureError::is_recoverable_device_loss_code(DXGI_ERROR_DEVICE_RESET));
        assert!(WindowsCaptureError::is_recoverable_device_loss_code(DXGI_ERROR_ACCESS_LOST));
        assert!(!WindowsCaptureError::is_recoverable_device_loss_code(HRESULT(
            0x80004005_u32 as _
        )));
    }

    #[test]
    fn nested_processing_error_preserves_device_loss_classification() {
        let error = WindowsCaptureError::FailedToProcessFrame(Box::new(
            WindowsCaptureError::FailedToMapTexture(windows_core::Error::from_hresult(
                DXGI_ERROR_DEVICE_RESET,
            )),
        ));

        assert!(error.is_recoverable_device_loss());
    }

    #[test]
    fn recreate_frame_pool_error_preserves_device_loss_classification() {
        let error = WindowsCaptureError::FailedToRecreateFramePool(
            windows_core::Error::from_hresult(DXGI_ERROR_DEVICE_RESET),
        );

        assert!(error.is_recoverable_device_loss());
    }

    #[test]
    fn recreate_capture_session_error_preserves_device_loss_classification() {
        let error = WindowsCaptureError::FailedToRecreateCaptureSession(
            windows_core::Error::from_hresult(DXGI_ERROR_DEVICE_RESET),
        );

        assert!(error.is_recoverable_device_loss());
    }
}

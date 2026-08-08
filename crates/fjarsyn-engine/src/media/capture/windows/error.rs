use windows::{
    Win32::Graphics::Dxgi::{
        DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED,
        DXGI_ERROR_DEVICE_RESET, DXGI_ERROR_DRIVER_INTERNAL_ERROR,
    },
    core::{Error as WindowsError, HRESULT},
};

pub(super) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    FailedToCreateFramePool(WindowsError),
    #[error("Failed to create capture session: {0}")]
    FailedToCreateCaptureSession(WindowsError),
    #[error("Failed to get capture item size: {0}")]
    FailedToGetCaptureItemSize(WindowsError),
    #[error("Failed to set min update interval: {0}")]
    FailedToSetMinUpdateInterval(WindowsError),
    #[error("Failed to set frame arrived handler: {0}")]
    FailedToSetFrameArrivedHandler(WindowsError),
    #[error("Failed to get surface: {0}")]
    FailedToGetSurface(WindowsError),
    #[error("Failed to get interface: {0}")]
    FailedToGetInterface(WindowsError),
    #[error("Failed to get content size: {0}")]
    FailedToGetContentSize(WindowsError),
    #[error("Failed to get device: {0}")]
    FailedToGetDevice(WindowsError),
    #[error("Failed to get immediate context: {0}")]
    FailedToGetImmediateContext(WindowsError),
    #[error("Failed to duplicate output: {0}")]
    DuplicateOutputFailed(WindowsError),
    #[error("Failed to create texture: {0}")]
    FailedToCreateTexture(WindowsError),
    #[error("Failed to recreate D3D device: {0}")]
    FailedToRecreateDevice(WindowsError),
    #[error("Failed to recreate WGC frame pool: {0}")]
    FailedToRecreateFramePool(WindowsError),
    #[error("Failed to recreate WGC capture session: {0}")]
    FailedToRecreateCaptureSession(WindowsError),
    #[error("Failed to map texture: {0}")]
    FailedToMapTexture(WindowsError),
    #[error("Readback buffer too small: expected at least {expected} bytes, got {actual}")]
    ReadbackBufferTooSmall { expected: usize, actual: usize },
    #[error("Failed to get monitor output: {0}")]
    FailedToGetMonitorOutput(WindowsError),
    #[error("Failed to create DispatcherQueueController: {0}")]
    FailedToCreateDispatcherQueueController(WindowsError),
    #[error("Failed to start capture: {0}")]
    FailedToStartCapture(WindowsError),
    #[error("Failed to start recreated capture session: {0}")]
    FailedToStartRecreatedCaptureSession(WindowsError),
    #[error("Failed to process frame")]
    FailedToProcessFrame(Box<Error>),
    #[error("Windows smart pointer cast failed: {0}")]
    CastFailed(WindowsError),
    #[error("Invalid staging depth, staging depth can't be less than 1")]
    InvalidStagingDepth,
    #[error("Frame sender closed")]
    FrameSenderClosed,
    #[error("Unknown Windows error: {0}")]
    Unknown(#[from] WindowsError),
}

impl Error {
    pub(super) fn is_recoverable_device_loss(&self) -> bool {
        match self {
            Self::FailedToProcessFrame(err) => err.is_recoverable_device_loss(),
            _ => self.windows_error().is_some_and(Self::is_recoverable_device_loss_error),
        }
    }

    pub(super) fn is_recoverable_device_loss_error(error: &WindowsError) -> bool {
        Self::is_recoverable_device_loss_code(error.code())
    }

    fn is_recoverable_device_loss_code(code: HRESULT) -> bool {
        code == DXGI_ERROR_ACCESS_LOST
            || code == DXGI_ERROR_DEVICE_HUNG
            || code == DXGI_ERROR_DEVICE_REMOVED
            || code == DXGI_ERROR_DEVICE_RESET
            || code == DXGI_ERROR_DRIVER_INTERNAL_ERROR
    }

    fn windows_error(&self) -> Option<&WindowsError> {
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
            | Self::Unknown(err) => Some(err),
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
        assert!(Error::is_recoverable_device_loss_code(DXGI_ERROR_DEVICE_REMOVED));
        assert!(Error::is_recoverable_device_loss_code(DXGI_ERROR_DEVICE_RESET));
        assert!(Error::is_recoverable_device_loss_code(DXGI_ERROR_ACCESS_LOST));
        assert!(!Error::is_recoverable_device_loss_code(HRESULT(0x80004005_u32 as _)));
    }

    #[test]
    fn nested_processing_error_preserves_device_loss_classification() {
        let error = Error::FailedToProcessFrame(Box::new(Error::FailedToMapTexture(
            WindowsError::from_hresult(DXGI_ERROR_DEVICE_RESET),
        )));

        assert!(error.is_recoverable_device_loss());
    }

    #[test]
    fn recreate_frame_pool_error_preserves_device_loss_classification() {
        let error =
            Error::FailedToRecreateFramePool(WindowsError::from_hresult(DXGI_ERROR_DEVICE_RESET));

        assert!(error.is_recoverable_device_loss());
    }

    #[test]
    fn recreate_capture_session_error_preserves_device_loss_classification() {
        let error = Error::FailedToRecreateCaptureSession(WindowsError::from_hresult(
            DXGI_ERROR_DEVICE_RESET,
        ));

        assert!(error.is_recoverable_device_loss());
    }
}

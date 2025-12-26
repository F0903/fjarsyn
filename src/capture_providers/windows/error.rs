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
    #[error("Failed to map texture: {0}")]
    FailedToMapTexture(windows_core::Error),
    #[error("Failed to get monitor output: {0}")]
    FailedToGetMonitorOutput(windows_core::Error),
    #[error("Failed to create DispatcherQueueController: {0}")]
    FailedToCreateDispatcherQueueController(windows_core::Error),
    #[error("Failed to start capture: {0}")]
    FailedToStartCapture(windows_core::Error),
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

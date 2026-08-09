#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("the frame has no importable GPU resource")]
    NoGpuResource,
    #[error("{0:?} GPU frames cannot be sampled by the desktop renderer")]
    UnsupportedFormat(crate::media::PixelFormat),
    #[error("invalid GPU frame dimensions {width}x{height}")]
    InvalidDimensions { width: i32, height: i32 },
    #[error("the desktop renderer is not using the Direct3D 12 backend")]
    UnsupportedBackend,
    #[error("failed to identify the D3D12 device that owns the renderer queue: {0}")]
    InspectQueueDevice(#[source] windows::core::Error),
    #[error("the renderer device and queue belong to different D3D12 devices")]
    DeviceQueueMismatch,
    #[error("the imported texture does not back this GPU frame")]
    FrameTextureMismatch,
    #[error("failed to open the shared GPU texture: {0}")]
    OpenTexture(#[source] windows::core::Error),
    #[error("failed to open the shared producer fence: {0}")]
    OpenFence(#[source] windows::core::Error),
    #[error("the shared GPU texture does not match its frame descriptor: {0}")]
    DescriptorMismatch(String),
    #[error("failed to enqueue the producer-fence wait: {0}")]
    WaitForProducer(#[source] windows::core::Error),
}

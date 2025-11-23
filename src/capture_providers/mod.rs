use futures::Stream;

pub mod shared;
pub mod windows;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("Failed to initialize")]
    FailedToInitialize,
    #[error("Already capturing")]
    AlreadyCapturing,
    #[error("Not capturing")]
    NotCapturing,
    #[error("No frame pool available")]
    NoFramePool,
    #[error("No capture item available")]
    NoCaptureItem,
    #[error("Already initialized")]
    AlreadyInitialized,
    #[error("Windows error: {0}")]
    WindowsError(#[from] windows_core::Error),
}

type CaptureResult<T> = std::result::Result<T, CaptureError>;

//TODO: use this instead of specifying the windows one. This also allows for other platforms to be added in the future.
trait CaptureProvider {
    type Frame;
    type Error;
    type Stream<'a>: Stream<Item = Result<Self::Frame, Self::Error>> + 'a
    where
        Self: 'a;

    /// Starts capture (allocate devices, sessions, etc.) and returns a stream of frames.
    fn capture<'a>(&'a mut self) -> Result<Self::Stream<'a>, Self::Error>;
}

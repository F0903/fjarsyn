pub mod capture_providers;
pub mod config;
pub mod media;
pub mod networking;
pub mod ui;
pub mod utils;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Capture error: {0}")]
    CaptureError(#[from] capture_providers::CaptureError),
    #[error("Windows capture builder error: {0}")]
    WindowsCaptureBuilderError(#[from] capture_providers::windows::WgcCaptureProviderBuilderError),
    #[error("Windows capture error: {0}")]
    WindowsError(#[from] windows_core::Error),
    #[error("UI error: {0}")]
    UiError(#[from] iced::Error),
    #[error("UI window management error: {0}")]
    UiWindowMgmtError(#[from] iced_winit::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("WebRTC error: {0}")]
    WebRtcError(#[from] crate::networking::webrtc::WebRTCError),
    #[error("Other error: {0}")]
    OtherError(#[from] Box<dyn std::error::Error>),
}

pub type Result<T> = std::result::Result<T, Error>;

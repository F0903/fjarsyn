pub mod ui;
pub mod utils;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Capture error: {0}")]
    CaptureError(#[from] fjarsyn_core::capture_providers::CaptureError),
    #[error("Windows capture builder error: {0}")]
    WindowsCaptureBuilderError(
        #[from] fjarsyn_core::capture_providers::windows::WgcCaptureProviderBuilderError,
    ),
    #[error("Windows capture error: {0}")]
    WindowsError(#[from] windows_core::Error),
    #[error("UI error: {0}")]
    UiError(#[from] iced::Error),
    #[error("UI window management error: {0}")]
    UiWindowMgmtError(#[from] iced_winit::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("WebRTC error: {0}")]
    WebRtcError(#[from] fjarsyn_core::networking::webrtc::WebRTCError),
    #[error("Config error: {0}")]
    ConfigError(#[from] fjarsyn_core::config::ConfigError),
}

pub type Result<T> = std::result::Result<T, Error>;

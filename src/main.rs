use std::sync::Arc;
use tokio::sync::Mutex;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

use crate::capture_providers::CaptureError;

mod capture_providers;
mod ui;
mod utils;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Capture error: {0}")]
    CaptureError(#[from] capture_providers::CaptureError),
    #[error("Windows capture builder error: {0}")]
    WindowsCaptureBuilderError(#[from] capture_providers::windows::BuilderError),
    #[error("Windows capture error: {0}")]
    WindowsError(#[from] windows_core::Error),
    #[error("UI error: {0}")]
    UiError(#[from] iced::Error),
    #[error("UI window management error: {0}")]
    UiWindowMgmtError(#[from] iced_winit::Error),
    #[error("Other error: {0}")]
    OtherError(#[from] Box<dyn std::error::Error>),
}

fn main() -> Result<()> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? }

    let windows_capture = capture_providers::windows::WindowsCaptureProviderBuilder::new()
        .with_default_device()?
        .with_default_capture_item()?
        .build()?;
    let windows_capture = Arc::new(Mutex::new(windows_capture));

    let app = ui::App::new(windows_capture)?;
    app.run();
    Ok(())
}

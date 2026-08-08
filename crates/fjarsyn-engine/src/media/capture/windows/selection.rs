//! Windows capture-item picker interop.

use std::{future::Future, pin::Pin, time::Duration};

use windows::{
    Foundation::AsyncStatus,
    Graphics::Capture::{GraphicsCaptureItem, GraphicsCapturePicker},
    Win32::{Foundation::HWND, UI::Shell::IInitializeWithWindow},
    core::{HRESULT, Interface, Result},
};

pub(in crate::media::capture) type PickCaptureItemFuture =
    Pin<Box<dyn Future<Output = Result<Option<GraphicsCaptureItem>>> + Send>>;

const INITIAL_POLL_INTERVAL: Duration = Duration::from_millis(1);
const MAX_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Shows a dialog in the specified window to pick an item to capture.
/// Returned future completes when the user picks an item or cancels the dialog.
pub(in crate::media::capture) fn user_pick_capture_item(
    raw_window_handle: u64,
) -> Result<PickCaptureItemFuture> {
    tracing::info!("Initializing GraphicsCapturePicker...");
    let picker = GraphicsCapturePicker::new()?;
    let init_with_window: IInitializeWithWindow = picker.cast()?;
    let window = HWND(raw_window_handle as usize as *mut core::ffi::c_void);
    unsafe { init_with_window.Initialize(window)? };

    tracing::info!("Waiting for user to pick capture item...");
    let item_future = async move {
        let operation = picker.PickSingleItemAsync()?;
        let mut poll_interval = INITIAL_POLL_INTERVAL;
        while operation.Status()? == AsyncStatus::Started {
            tokio::time::sleep(poll_interval).await;
            poll_interval = poll_interval.saturating_mul(2).min(MAX_POLL_INTERVAL);
        }

        let result = operation.GetResults();
        match &result {
            Ok(item) => tracing::info!(
                "User picked capture item: {:?}",
                item.DisplayName().unwrap_or_default()
            ),
            Err(error) => {
                if error.code() == HRESULT(0) {
                    return Ok(None);
                }
                tracing::error!("Error picking capture item: {:?}", error)
            }
        }
        result.map(Some)
    };
    Ok(Box::pin(item_future))
}

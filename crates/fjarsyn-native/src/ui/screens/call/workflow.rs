use fjarsyn_core::capture_providers::PlatformCaptureItem;

use super::{CallMessage, CallScreen, state::RemoteStreamStatus};
use crate::ui::app::AppContextMut;

#[derive(Debug, Clone)]
pub(crate) enum CallEffect {
    NotifyError(String),
    NotifyInfo(String),
    InitializeCapture,
    OpenCapturePicker { window_handle: u64 },
    RunCaptureStart { capture_item: PlatformCaptureItem },
    RunCaptureStop,
    StartLocalCapturePipeline,
    EndCall,
}

// The call reducer owns screen-local state transitions and emits runtime work
// for the Iced layer to interpret. That keeps the update loop focused on
// running effects instead of deciding what should happen next.
pub(crate) fn reduce(
    screen: &mut CallScreen,
    ctx: &mut AppContextMut<'_>,
    message: CallMessage,
) -> Vec<CallEffect> {
    match message {
        CallMessage::LocalFrameReady(frame) => {
            screen.local.latest_frame = Some(frame);
            Vec::new()
        }
        CallMessage::DecodedFrameReady(frame) => {
            if screen.remote.stream_status != RemoteStreamStatus::Inactive {
                screen.remote.latest_frame = Some(frame);
            }
            Vec::new()
        }
        CallMessage::DecodedFrameCleared => {
            screen.remote.latest_frame = None;
            Vec::new()
        }
        CallMessage::RemoteStreamStarted => {
            screen.remote.stream_status = RemoteStreamStatus::Active;
            Vec::new()
        }
        CallMessage::RemoteStreamEnded => {
            screen.remote.stream_status = RemoteStreamStatus::Inactive;
            screen.remote.latest_frame = None;
            Vec::new()
        }
        CallMessage::ToggleLocalPreview => {
            screen.local.preview_visible = !screen.local.preview_visible;
            Vec::new()
        }
        CallMessage::EndCall => {
            screen.clear_media_pipeline();
            screen.capture.pending_start = false;
            vec![CallEffect::EndCall]
        }
        CallMessage::StartCapture => reduce_start_capture(screen, ctx),
        CallMessage::PlatformUserPickedCaptureItem(result) => reduce_capture_item_picked(result),
        CallMessage::TryStartCapture(capture_item) => {
            vec![CallEffect::RunCaptureStart { capture_item }]
        }
        CallMessage::CaptureStarted => vec![CallEffect::StartLocalCapturePipeline],
        CallMessage::StopCapture | CallMessage::TryStopCapture => vec![CallEffect::RunCaptureStop],
        CallMessage::CaptureStopped => {
            screen.clear_local_media_pipeline();
            screen.capture.pending_start = false;
            Vec::new()
        }
    }
}

fn reduce_start_capture(screen: &mut CallScreen, ctx: &mut AppContextMut<'_>) -> Vec<CallEffect> {
    if screen.capture.provider.is_none() {
        screen.capture.pending_start = true;

        if ctx.media.capture_initializing {
            return Vec::new();
        }

        ctx.media.capture_initializing = true;
        return vec![
            CallEffect::NotifyInfo("Initializing screen capture...".into()),
            CallEffect::InitializeCapture,
        ];
    }

    match ctx.ui.main_window.as_ref().and_then(|window| window.raw_id) {
        Some(window_handle) => vec![CallEffect::OpenCapturePicker { window_handle }],
        None => vec![CallEffect::NotifyError(
            "Screen capture picker is unavailable without an active window.".into(),
        )],
    }
}

fn reduce_capture_item_picked(
    result: Result<Option<PlatformCaptureItem>, String>,
) -> Vec<CallEffect> {
    match result {
        Ok(Some(capture_item)) => vec![CallEffect::RunCaptureStart { capture_item }],
        Ok(None) => Vec::new(),
        Err(err) => vec![CallEffect::NotifyError(format!("Failed to pick capture item: {}", err))],
    }
}

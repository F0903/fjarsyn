//! Capture setup decisions and detached WGC cleanup.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::{
    media::{
        PixelFormat,
        capture::{PlatformProvider, Provider},
        gpu_interop,
    },
    screen_share::Config,
};

pub(super) struct CaptureGuard(Option<Arc<RwLock<PlatformProvider>>>);

impl CaptureGuard {
    pub(super) fn new(capture: Arc<RwLock<PlatformProvider>>) -> Self {
        Self(Some(capture))
    }

    pub(super) fn capture(&self) -> &Arc<RwLock<PlatformProvider>> {
        self.0.as_ref().expect("capture guard remains armed")
    }

    pub(super) fn disarm(mut self) -> Arc<RwLock<PlatformProvider>> {
        self.0.take().expect("capture guard remains armed")
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        if let Some(capture) = self.0.take() {
            stop_capture(capture);
        }
    }
}

pub(super) fn requires_capture_readback(config: &Config) -> bool {
    gpu_interop::requires_cpu_readback(
        config.capture.enable_ui_preview,
        PixelFormat::DEFAULT_CAPTURE,
        config.video.transcoding_type.uses_hardware_encoder(),
    )
}

/// Stops WGC away from Tokio because synchronous COM close calls may stall.
pub(super) fn stop_capture(capture: Arc<RwLock<PlatformProvider>>) {
    // Retain a second reference until thread creation succeeds. If the OS
    // refuses a new cleanup thread, intentionally leak this shutdown-only
    // reference rather than synchronously dropping a potentially stuck WGC
    // provider on the async runtime.
    let fallback = capture.clone();
    let spawn =
        std::thread::Builder::new().name("fjarsyn-capture-cleanup".into()).spawn(move || {
            use windows::Win32::System::Com::{
                COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
            };

            // SAFETY: this dedicated thread owns its COM apartment and pairs a
            // successful initialization with exactly one uninitialization.
            if let Err(error) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
                tracing::warn!(%error, "failed to initialize capture cleanup COM apartment; deferring cleanup to process exit");
                std::mem::forget(capture);
                return;
            }
            {
                let mut provider = capture.blocking_write();
                if let Err(error) = provider.stop_capture() {
                    tracing::warn!(%error, "failed to stop capture during detached cleanup");
                }
            }
            drop(capture);
            // SAFETY: COM was initialized successfully above on this thread.
            unsafe { CoUninitialize() };
        });
    if let Err(error) = spawn {
        tracing::warn!(%error, "failed to spawn capture cleanup thread; deferring cleanup to process exit");
        std::mem::forget(fallback);
    }
}

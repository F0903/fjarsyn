use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use windows::Graphics::Capture::GraphicsCaptureSession;

use crate::media::capture::windows::{Error, Result};

#[derive(Debug, Clone, Copy)]
pub(in crate::media::capture::windows::wgc) struct SessionSettings {
    pub(in crate::media::capture::windows::wgc) record_cursor: bool,
    pub(in crate::media::capture::windows::wgc) border_indicator: bool,
    pub(in crate::media::capture::windows::wgc) min_update_interval: Duration,
}

impl SessionSettings {
    pub(in crate::media::capture::windows::wgc) fn apply(
        self,
        session: &GraphicsCaptureSession,
    ) -> Result<()> {
        if let Err(error) = session.SetIsCursorCaptureEnabled(self.record_cursor) {
            tracing::warn!(%error, "failed to configure cursor capture");
        }
        if let Err(error) = session.SetIsBorderRequired(self.border_indicator) {
            tracing::warn!(%error, "failed to configure the capture border");
        }

        session.SetMinUpdateInterval(self.min_update_interval.into()).map_err(|error| {
            tracing::error!(%error, "failed to configure the minimum capture update interval");
            Error::FailedToSetMinUpdateInterval(error)
        })
    }
}

#[derive(Debug, Clone)]
struct SendableSession(GraphicsCaptureSession);

// WGC's free-threaded frame pool invokes callbacks off the UI thread. Keep the
// unsafe boundary attached to the exact WinRT handle shared with recovery.
unsafe impl Send for SendableSession {}
unsafe impl Sync for SendableSession {}

impl SendableSession {
    fn handle(&self) -> GraphicsCaptureSession {
        self.0.clone()
    }

    fn into_inner(self) -> GraphicsCaptureSession {
        self.0
    }
}

#[derive(Debug, Clone)]
pub(in crate::media::capture::windows::wgc) struct SessionState {
    inner: Arc<RwLock<Option<SendableSession>>>,
}

impl SessionState {
    pub(in crate::media::capture::windows::wgc) fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(None)) }
    }

    pub(in crate::media::capture::windows::wgc) fn get(&self) -> Option<GraphicsCaptureSession> {
        self.inner.read().unwrap().as_ref().map(SendableSession::handle)
    }

    pub(in crate::media::capture::windows::wgc) fn replace(&self, session: GraphicsCaptureSession) {
        *self.inner.write().unwrap() = Some(SendableSession(session));
    }

    pub(in crate::media::capture::windows::wgc) fn take(&self) -> Option<GraphicsCaptureSession> {
        self.inner.write().unwrap().take().map(SendableSession::into_inner)
    }
}

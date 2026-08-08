use std::sync::{Arc, Mutex, RwLock};

use windows::Graphics::{
    Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
    SizeInt32,
};

use super::{DeviceState, ResourcePool, SessionSettings, SessionState};
use crate::media::PixelFormat;

#[derive(Clone)]
pub(in crate::media::capture::windows::wgc) struct DeviceLossRecovery {
    device: DeviceState,
    capture_item: GraphicsCaptureItem,
    active_session: SessionState,
    pixel_format: PixelFormat,
    settings: SessionSettings,
    resources: Arc<RwLock<ResourcePool>>,
    lock: Arc<Mutex<()>>,
}

impl DeviceLossRecovery {
    pub(in crate::media::capture::windows::wgc) fn new(
        device: DeviceState,
        capture_item: GraphicsCaptureItem,
        active_session: SessionState,
        pixel_format: PixelFormat,
        settings: SessionSettings,
        resources: Arc<RwLock<ResourcePool>>,
    ) -> Self {
        Self {
            device,
            capture_item,
            active_session,
            pixel_format,
            settings,
            resources,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub(in crate::media::capture::windows::wgc) fn recover(
        &self,
        frame_pool: &Direct3D11CaptureFramePool,
        size: SizeInt32,
    ) -> bool {
        let Ok(_guard) = self.lock.try_lock() else {
            tracing::debug!("skipping WGC device-loss recovery because another recovery is active");
            return false;
        };

        let result = self.device.rebuild_capture_stack(
            frame_pool,
            &self.capture_item,
            &self.active_session,
            self.pixel_format,
            size,
            self.settings,
        );
        self.resources.write().unwrap().reset();
        match result {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(%error, "failed to recreate WGC device resources");
                false
            }
        }
    }

    pub(in crate::media::capture::windows::wgc) fn device(&self) -> &DeviceState {
        &self.device
    }

    pub(in crate::media::capture::windows::wgc) fn resources(&self) -> Arc<RwLock<ResourcePool>> {
        self.resources.clone()
    }

    pub(in crate::media::capture::windows::wgc) fn reset_resources(&self) {
        self.resources.write().unwrap().reset();
    }
}

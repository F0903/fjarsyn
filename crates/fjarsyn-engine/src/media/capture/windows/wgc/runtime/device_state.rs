use std::sync::{Arc, RwLock, RwLockReadGuard};

use windows::{
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::Direct3D11::IDirect3DDevice,
        SizeInt32,
    },
    core::{IUnknown, Interface},
};

use super::{FRAME_BUFFER_COUNT, SessionSettings, SessionState};
use crate::media::{
    PixelFormat,
    capture::windows::{Error, Result, create_d3d_device, native_to_winrt_d3d11device},
};

#[derive(Debug, Clone)]
struct SendableDevice(IDirect3DDevice);

// WGC's free-threaded frame pool invokes callbacks off the UI thread. Keep the
// unsafe boundary attached to the exact WinRT handle shared with recovery.
unsafe impl Send for SendableDevice {}
unsafe impl Sync for SendableDevice {}

impl SendableDevice {
    fn handle(&self) -> IDirect3DDevice {
        self.0.clone()
    }
}

#[derive(Debug, Clone)]
pub(in crate::media::capture::windows::wgc) struct DeviceState {
    inner: Arc<RwLock<SendableDevice>>,
}

pub(in crate::media::capture::windows::wgc) struct CurrentDevice<'a> {
    inner: RwLockReadGuard<'a, SendableDevice>,
}

impl CurrentDevice<'_> {
    pub(in crate::media::capture::windows::wgc) fn matches_native(
        &self,
        device: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
    ) -> windows::core::Result<bool> {
        let current = crate::media::capture::windows::winrt_to_native_d3d11device(&self.inner.0)?;
        let current: IUnknown = current.cast()?;
        let candidate: IUnknown = device.cast()?;
        Ok(current.as_raw() == candidate.as_raw())
    }
}

impl DeviceState {
    pub(in crate::media::capture::windows::wgc) fn new(device: IDirect3DDevice) -> Self {
        Self { inner: Arc::new(RwLock::new(SendableDevice(device))) }
    }

    pub(in crate::media::capture::windows::wgc) fn get(&self) -> IDirect3DDevice {
        self.inner.read().unwrap().handle()
    }

    pub(in crate::media::capture::windows::wgc) fn current(&self) -> CurrentDevice<'_> {
        CurrentDevice { inner: self.inner.read().unwrap() }
    }

    fn replace(&self, device: IDirect3DDevice) {
        *self.inner.write().unwrap() = SendableDevice(device);
    }

    pub(in crate::media::capture::windows::wgc) fn resize_frame_pool(
        &self,
        frame_pool: &Direct3D11CaptureFramePool,
        pixel_format: PixelFormat,
        size: SizeInt32,
    ) -> windows::core::Result<()> {
        frame_pool.Recreate(
            &self.get(),
            pixel_format.to_directx_pixel_format(),
            FRAME_BUFFER_COUNT,
            size,
        )
    }

    pub(super) fn rebuild_capture_stack(
        &self,
        frame_pool: &Direct3D11CaptureFramePool,
        capture_item: &GraphicsCaptureItem,
        active_session: &SessionState,
        pixel_format: PixelFormat,
        size: SizeInt32,
        settings: SessionSettings,
    ) -> Result<()> {
        let d3d_device = create_d3d_device().map_err(Error::FailedToRecreateDevice)?;
        let winrt_device =
            native_to_winrt_d3d11device(&d3d_device).map_err(Error::FailedToRecreateDevice)?;

        frame_pool
            .Recreate(
                &winrt_device,
                pixel_format.to_directx_pixel_format(),
                FRAME_BUFFER_COUNT,
                size,
            )
            .map_err(Error::FailedToRecreateFramePool)?;

        if let Some(session) = active_session.take() {
            session.Close().ok();
        }

        let session = frame_pool
            .CreateCaptureSession(capture_item)
            .map_err(Error::FailedToRecreateCaptureSession)?;
        settings.apply(&session)?;
        session.StartCapture().map_err(Error::FailedToStartRecreatedCaptureSession)?;
        active_session.replace(session);
        self.replace(winrt_device);
        Ok(())
    }
}

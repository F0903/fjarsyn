use std::sync::{Arc, Mutex, RwLock};

use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        SizeInt32,
    },
};

use super::{
    super::WindowsCaptureError, CaptureSessionSettings, ResourcePool, Result, WgcCaptureProvider,
    WgcDeviceState, WgcSessionState,
};
use crate::{
    capture_providers::{
        CaptureFramerate, CaptureProvider,
        windows::d3d11_utils::{create_d3d_device, native_to_winrt_d3d11device},
    },
    media::pixel_format::PixelFormat,
};

impl WgcDeviceState {
    fn resize_frame_pool(
        &self,
        frame_pool: &Direct3D11CaptureFramePool,
        pixel_format: PixelFormat,
        size: SizeInt32,
    ) -> windows_core::Result<()> {
        let device = self.get();
        frame_pool.Recreate(
            &device,
            pixel_format.to_directx_pixel_format(),
            WgcCaptureProvider::WGC_FRAME_BUFFERS,
            size,
        )
    }

    fn rebuild_capture_stack(
        &self,
        frame_pool: &Direct3D11CaptureFramePool,
        capture_item: &GraphicsCaptureItem,
        active_session: &WgcSessionState,
        pixel_format: PixelFormat,
        size: SizeInt32,
        settings: CaptureSessionSettings,
    ) -> Result<()> {
        let d3d_device =
            create_d3d_device().map_err(WindowsCaptureError::FailedToRecreateDevice)?;
        let winrt_device = native_to_winrt_d3d11device(&d3d_device)
            .map_err(WindowsCaptureError::FailedToRecreateDevice)?;

        frame_pool
            .Recreate(
                &winrt_device,
                pixel_format.to_directx_pixel_format(),
                WgcCaptureProvider::WGC_FRAME_BUFFERS,
                size,
            )
            .map_err(WindowsCaptureError::FailedToRecreateFramePool)?;

        if let Some(session) = active_session.take() {
            session.Close().ok();
        }

        let session = frame_pool
            .CreateCaptureSession(capture_item)
            .map_err(WindowsCaptureError::FailedToRecreateCaptureSession)?;
        configure_capture_session(&session, settings)?;
        session
            .StartCapture()
            .map_err(WindowsCaptureError::FailedToStartRecreatedCaptureSession)?;
        active_session.replace(session);
        self.replace(winrt_device);
        Ok(())
    }
}

#[derive(Clone)]
struct DeviceLossRecoveryContext {
    device: WgcDeviceState,
    capture_item: GraphicsCaptureItem,
    active_session: WgcSessionState,
    pixel_format: PixelFormat,
    settings: CaptureSessionSettings,
    resource_state: Arc<RwLock<ResourcePool>>,
    recovery_lock: Arc<Mutex<()>>,
}

impl DeviceLossRecoveryContext {
    fn recover(&self, frame_pool: &Direct3D11CaptureFramePool, size: SizeInt32) -> bool {
        let Ok(_guard) = self.recovery_lock.try_lock() else {
            tracing::debug!(
                "Skipping WGC device-loss recovery because another recovery is active."
            );
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
        WgcCaptureProvider::reset_resource_pool(&self.resource_state);
        match result {
            Ok(()) => true,
            Err(err) => {
                tracing::error!("Failed to recreate WGC device resources: {}", err);
                false
            }
        }
    }
}

impl CaptureProvider for WgcCaptureProvider {
    type Result<T> = Result<T>;
    type Stream = super::WindowsCaptureStream;
    type CaptureItem = GraphicsCaptureItem;

    fn create_stream(&mut self, framerate: CaptureFramerate) -> Self::Result<Self::Stream> {
        self.stop_capture().ok();

        let (tx, rx) = tokio::sync::mpsc::channel(Self::PIPELINE_DEPTH);

        let capture_item = self.capture_item.as_ref().ok_or_else(|| {
            tracing::error!("No capture item set!");
            super::super::WindowsCaptureError::NoCaptureItem
        })?;

        let device = self.device.get();
        let resource_state_arc = self.resource_state.clone();

        let size = capture_item.Size().map_err(|e| {
            tracing::error!("Failed to get size of capture item! {}", e);
            super::super::WindowsCaptureError::FailedToGetCaptureItemSize(e)
        })?;

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device,
            self.pixel_format.to_directx_pixel_format(),
            Self::WGC_FRAME_BUFFERS,
            size,
        )
        .map_err(|e| {
            tracing::error!("Failed to create frame pool! {}", e);
            super::super::WindowsCaptureError::FailedToCreateFramePool(e)
        })?;

        let session = frame_pool.CreateCaptureSession(capture_item).map_err(|e| {
            tracing::error!("Failed to create capture session! {}", e);
            super::super::WindowsCaptureError::FailedToCreateCaptureSession(e)
        })?;
        let session_settings = CaptureSessionSettings {
            record_cursor: self.record_cursor,
            border_indicator: self.border_indicator,
            min_update_interval: framerate.to_frametime(),
        };
        configure_capture_session(&session, session_settings)?;

        let buffer_pool = self.buffer_pool.clone();
        let capture_options = self.capture_options.clone();
        let pixel_format = self.pixel_format;
        let recovery = DeviceLossRecoveryContext {
            device: self.device.clone(),
            capture_item: capture_item.clone(),
            active_session: self.session.clone(),
            pixel_format,
            settings: session_settings,
            resource_state: resource_state_arc.clone(),
            recovery_lock: Arc::new(Mutex::new(())),
        };
        let mut frame_pool_size = size;

        let token = frame_pool
            .FrameArrived(&TypedEventHandler::new(move |sender, _| {
                if tx.is_closed() {
                    return Ok(());
                }

                let sender: &Direct3D11CaptureFramePool = match sender {
                    Some(sender) => sender,
                    None => return Ok(()),
                };

                let mut frame = match sender.TryGetNextFrame() {
                    Ok(frame) => frame,
                    Err(err) => {
                        if WindowsCaptureError::is_recoverable_device_loss_error(&err) {
                            tracing::warn!(
                                "Capture device/access lost while getting the next frame; recreating WGC device resources."
                            );
                            let _ = recovery.recover(sender, frame_pool_size);
                        }
                        tracing::error!("Failed to get next frame: {}", err);
                        return Ok(());
                    }
                };

                let mut skipped_frames = 0usize;
                while let Ok(next_frame) = sender.TryGetNextFrame() {
                    frame = next_frame;
                    skipped_frames += 1;
                }

                if skipped_frames > 0 {
                    tracing::trace!(
                        "Dropped {} stale capture frames before processing the latest frame.",
                        skipped_frames
                    );
                }

                let content_size = frame.ContentSize().unwrap_or(frame_pool_size);
                if !same_capture_size(content_size, frame_pool_size) {
                    tracing::info!(
                        "Capture content resized from {}x{} to {}x{}; recreating frame pool.",
                        frame_pool_size.Width,
                        frame_pool_size.Height,
                        content_size.Width,
                        content_size.Height
                    );

                    if let Err(err) =
                        recovery.device.resize_frame_pool(sender, pixel_format, content_size)
                    {
                        if WindowsCaptureError::is_recoverable_device_loss_error(&err) {
                            tracing::warn!(
                                "Capture device/access lost while recreating the frame pool; recreating WGC device resources."
                            );
                            if recovery.recover(sender, content_size) {
                                frame_pool_size = content_size;
                            } else {
                                tracing::error!("Failed to recreate capture frame pool: {}", err);
                            }
                        } else {
                            tracing::error!("Failed to recreate capture frame pool: {}", err);
                        }
                        return Ok(());
                    }

                    Self::reset_resource_pool(&recovery.resource_state);
                    frame_pool_size = content_size;
                    return Ok(());
                }

                let mut buffer = None;
                let capture_options = *capture_options.read().unwrap();

                if capture_options.cpu_readback_enabled {
                    let buffer_size = content_size.Width as usize
                        * content_size.Height as usize
                        * pixel_format.bytes_per_pixel() as usize;

                    if buffer_size > 0 {
                        let mut new_buffer = buffer_pool.get_unzeroed(buffer_size);
                        unsafe {
                            new_buffer.set_len(buffer_size);
                        }
                        buffer = Some(new_buffer);
                    }
                }

                match Self::process_frame(
                    buffer,
                    frame,
                    recovery.resource_state.clone(),
                    pixel_format,
                    framerate.to_frametime(),
                    tx.clone(),
                ) {
                    Ok(()) | Err(super::super::WindowsCaptureError::FrameSenderClosed) => {}
                    Err(err) if err.is_recoverable_device_loss() => {
                        tracing::warn!(
                            "Capture device/access lost while processing a frame; recreating WGC device resources."
                        );
                        let _ = recovery.recover(sender, content_size);
                    }
                    Err(err) => {
                        tracing::error!("Failed to process frame: {}", err);
                    }
                }

                Ok(())
            }))
            .map_err(|e| {
                tracing::error!("Failed to set FrameArrived handler! {}", e);
                super::super::WindowsCaptureError::FailedToSetFrameArrivedHandler(e)
            })?;
        tracing::debug!("Added frame arrived handler with token: {:?}", token);
        self.stream_tokens.push(token);

        session.StartCapture().map_err(|e| {
            tracing::error!("Failed to start capture! {}", e);
            super::super::WindowsCaptureError::FailedToStartCapture(e)
        })?;

        self.capturing = true;
        self.frame_pool = Some(frame_pool);
        self.session.replace(session);

        Ok(super::WindowsCaptureStream::new(rx))
    }

    fn set_capture_item(&mut self, capture_item: Self::CaptureItem) -> Self::Result<()> {
        tracing::info!(
            "Setting capture item: {}",
            capture_item.DisplayName().unwrap_or("<no name>".into())
        );
        self.capture_item = Some(capture_item);
        Self::reset_resource_pool(&self.resource_state);

        Ok(())
    }

    fn start_capture(&mut self) -> Self::Result<()> {
        if self.capturing {
            tracing::warn!("Tried to start capture, but was already capturing.");
            return Ok(());
        }

        if self.capture_item.is_none() {
            tracing::error!("No capture item set!");
            return Err(super::super::WindowsCaptureError::NoCaptureItem);
        }

        if let Some(session) = self.session.get() {
            session.StartCapture().map_err(|e| {
                tracing::error!("Failed to start capture! {}", e);
                super::super::WindowsCaptureError::FailedToStartCapture(e)
            })?;
        }

        self.capturing = true;
        Ok(())
    }

    fn stop_capture(&mut self) -> Self::Result<()> {
        tracing::info!("Stopping capture session...");
        if !self.capturing {
            tracing::info!("Capture already stopped.");
            return Ok(());
        }

        if let Some(session) = self.session.take() {
            tracing::info!("Closing GraphicsCaptureSession");
            session.Close().ok();
        }
        if let Some(frame_pool) = &self.frame_pool {
            tracing::info!("Closing Direct3D11CaptureFramePool");
            for token in self.stream_tokens.drain(..) {
                tracing::debug!("Removing frame arrived handler: {:?}", token);
                frame_pool.RemoveFrameArrived(token).ok();
            }
            frame_pool.Close().ok();
        }

        self.frame_pool = None;
        self.capturing = false;
        tracing::info!("Capture session stopped successfully.");
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }

    fn codec_device(&self) -> Option<crate::media::CodecDeviceLease> {
        let device = self.device.get();
        super::super::d3d11_utils::winrt_to_native_d3d11device(&device)
            .ok()
            .map(crate::media::CodecDeviceLease::from_d3d11)
    }
}

fn configure_capture_session(
    session: &GraphicsCaptureSession,
    settings: CaptureSessionSettings,
) -> Result<()> {
    if let Err(err) = session.SetIsCursorCaptureEnabled(settings.record_cursor) {
        tracing::warn!("Failed to set IsCursorCaptureEnabled: {}", err);
    }
    if let Err(err) = session.SetIsBorderRequired(settings.border_indicator) {
        tracing::warn!("Failed to set IsBorderRequired: {}", err);
    }

    session.SetMinUpdateInterval(settings.min_update_interval.into()).map_err(|e| {
        tracing::error!("Failed to set MinUpdateInterval: {}", e);
        super::super::WindowsCaptureError::FailedToSetMinUpdateInterval(e)
    })
}

fn same_capture_size(left: SizeInt32, right: SizeInt32) -> bool {
    left.Width == right.Width && left.Height == right.Height
}

impl Drop for WgcCaptureProvider {
    fn drop(&mut self) {
        self.stop_capture().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_capture_size_compares_dimensions() {
        assert!(same_capture_size(
            SizeInt32 { Width: 1920, Height: 1080 },
            SizeInt32 { Width: 1920, Height: 1080 },
        ));
        assert!(!same_capture_size(
            SizeInt32 { Width: 1920, Height: 1080 },
            SizeInt32 { Width: 1280, Height: 720 },
        ));
    }
}

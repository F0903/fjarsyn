use windows::{
    Foundation::TypedEventHandler,
    Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
};

use super::{Result, WgcCaptureProvider};
use crate::capture_providers::{CaptureFramerate, CaptureProvider};

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

        let device = self.device.clone();
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

        if let Err(err) = session.SetIsCursorCaptureEnabled(self.record_cursor) {
            tracing::warn!("Failed to set IsCursorCaptureEnabled: {}", err);
        }
        if let Err(err) = session.SetIsBorderRequired(self.border_indicator) {
            tracing::warn!("Failed to set IsBorderRequired: {}", err);
        }

        session.SetMinUpdateInterval(framerate.to_frametime().into()).map_err(|e| {
            tracing::error!("Failed to set MinUpdateInterval: {}", e);
            super::super::WindowsCaptureError::FailedToSetMinUpdateInterval(e)
        })?;

        let buffer_pool = self.buffer_pool.clone();
        let resource_state_arc_inner = resource_state_arc.clone();
        let capture_options = self.capture_options.clone();
        let pixel_format = self.pixel_format;

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

                let content_size = frame.ContentSize().unwrap_or(size);
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
                    resource_state_arc_inner.clone(),
                    pixel_format,
                    framerate.to_frametime(),
                    tx.clone(),
                ) {
                    Ok(()) | Err(super::super::WindowsCaptureError::FrameSenderClosed) => {}
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
        self.session = Some(session);

        Ok(super::WindowsCaptureStream::new(rx))
    }

    fn set_capture_item(&mut self, capture_item: Self::CaptureItem) -> Self::Result<()> {
        tracing::info!(
            "Setting capture item: {}",
            capture_item.DisplayName().unwrap_or("<no name>".into())
        );
        self.capture_item = Some(capture_item);

        {
            let mut state = self.resource_state.write().unwrap();
            state.shared_textures.clear();
            state.shared_handles.clear();
            state.staging_textures.clear();
            state.frame_count = 0;
            state.last_emitted_timestamp_100ns = None;
        }

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

        if let Some(session) = &self.session {
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

        if let Some(session) = &self.session {
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

        self.session = None;
        self.frame_pool = None;
        self.capturing = false;
        tracing::info!("Capture session stopped successfully.");
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }

    fn raw_device_handle(&self) -> Option<*mut std::ffi::c_void> {
        super::super::d3d11_utils::winrt_to_native_d3d11device(&self.device).ok().map(|device| {
            let device = std::mem::ManuallyDrop::new(device);
            windows_core::Interface::as_raw(&*device)
        })
    }
}

impl Drop for WgcCaptureProvider {
    fn drop(&mut self) {
        self.stop_capture().ok();
    }
}

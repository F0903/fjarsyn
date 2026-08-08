use std::sync::{Arc, RwLock};

use futures::Stream as FuturesStream;
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::Direct3D11::IDirect3DDevice,
        SizeInt32,
    },
};

use super::runtime::{
    DeviceLossRecovery, DeviceState, FRAME_BUFFER_COUNT, PIPELINE_DEPTH, ResourcePool,
    SessionSettings, SessionState, process_frame,
};
use crate::media::{
    PixelFormat,
    buffer_pool::Pool,
    capture::{
        Provider as ProviderContract,
        windows::{Error, Result},
    },
    frame::Frame,
    video::Framerate,
};

#[derive(Debug)]
pub struct Stream {
    channel: tokio::sync::mpsc::Receiver<Frame>,
}

impl Stream {
    fn new(channel: tokio::sync::mpsc::Receiver<Frame>) -> Self {
        Self { channel }
    }
}

impl FuturesStream for Stream {
    type Item = Frame;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.channel.poll_recv(cx)
    }
}

#[derive(Debug)]
pub struct Provider {
    pub(super) device: DeviceState,
    pub(super) capture_item: Option<GraphicsCaptureItem>,
    pub(super) pixel_format: PixelFormat,
    pub(super) resources: Arc<RwLock<ResourcePool>>,
    pub(super) cpu_readback_enabled: bool,
    pub(super) buffer_pool: Pool,

    pub(super) frame_pool: Option<Direct3D11CaptureFramePool>,
    pub(super) session: SessionState,
    pub(super) stream_tokens: Vec<windows::Foundation::EventRegistrationToken>,
    pub(super) capturing: bool,

    pub(super) record_cursor: bool,
    pub(super) border_indicator: bool,
}

impl Provider {
    const BUFFER_SIZE: usize = 16 * 1024 * 1024;
    const BUFFER_MAX_COUNT: usize = 8;

    pub fn new(
        device: IDirect3DDevice,
        pixel_format: PixelFormat,
        record_cursor: bool,
        border_indicator: bool,
        cpu_readback_enabled: bool,
    ) -> Self {
        Self {
            device: DeviceState::new(device),
            capture_item: None,
            pixel_format,
            resources: Arc::new(RwLock::new(ResourcePool::default())),
            cpu_readback_enabled,
            buffer_pool: Pool::new(Self::BUFFER_SIZE, Self::BUFFER_MAX_COUNT),
            frame_pool: None,
            session: SessionState::new(),
            stream_tokens: Vec::new(),
            capturing: false,
            record_cursor,
            border_indicator,
        }
    }
}

impl ProviderContract for Provider {
    type Result<T> = Result<T>;
    type Stream = Stream;
    type Item = GraphicsCaptureItem;

    fn create_stream(&mut self, framerate: Framerate) -> Self::Result<Self::Stream> {
        self.stop_capture().ok();

        let (tx, rx) = tokio::sync::mpsc::channel(PIPELINE_DEPTH);

        let capture_item = self.capture_item.as_ref().ok_or_else(|| {
            tracing::error!("No capture item set!");
            Error::NoCaptureItem
        })?;

        let device = self.device.get();
        let resources = self.resources.clone();

        let size = capture_item.Size().map_err(|e| {
            tracing::error!("Failed to get size of capture item! {}", e);
            Error::FailedToGetCaptureItemSize(e)
        })?;

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device,
            self.pixel_format.to_directx_pixel_format(),
            FRAME_BUFFER_COUNT,
            size,
        )
        .map_err(|e| {
            tracing::error!("Failed to create frame pool! {}", e);
            Error::FailedToCreateFramePool(e)
        })?;

        let session = frame_pool.CreateCaptureSession(capture_item).map_err(|e| {
            tracing::error!("Failed to create capture session! {}", e);
            Error::FailedToCreateCaptureSession(e)
        })?;
        let session_settings = SessionSettings {
            record_cursor: self.record_cursor,
            border_indicator: self.border_indicator,
            min_update_interval: framerate.to_frametime(),
        };
        session_settings.apply(&session)?;

        let buffer_pool = self.buffer_pool.clone();
        let cpu_readback_enabled = self.cpu_readback_enabled;
        let pixel_format = self.pixel_format;
        let recovery = DeviceLossRecovery::new(
            self.device.clone(),
            capture_item.clone(),
            self.session.clone(),
            pixel_format,
            session_settings,
            resources.clone(),
        );
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
                        if Error::is_recoverable_device_loss_error(&err) {
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
                        recovery
                            .device()
                            .resize_frame_pool(sender, pixel_format, content_size)
                    {
                        if Error::is_recoverable_device_loss_error(&err) {
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

                    recovery.reset_resources();
                    frame_pool_size = content_size;
                    return Ok(());
                }

                let mut buffer = None;
                if cpu_readback_enabled {
                    let buffer_size = content_size.Width as usize
                        * content_size.Height as usize
                        * pixel_format.bytes_per_pixel() as usize;

                    if buffer_size > 0 {
                        buffer = Some(buffer_pool.get(buffer_size));
                    }
                }

                match process_frame(
                    buffer,
                    frame,
                    recovery.resources(),
                    pixel_format,
                    framerate.to_frametime(),
                    tx.clone(),
                ) {
                    Ok(()) | Err(Error::FrameSenderClosed) => {}
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
                Error::FailedToSetFrameArrivedHandler(e)
            })?;
        tracing::debug!("Added frame arrived handler with token: {:?}", token);
        self.stream_tokens.push(token);

        session.StartCapture().map_err(|e| {
            tracing::error!("Failed to start capture! {}", e);
            Error::FailedToStartCapture(e)
        })?;

        self.capturing = true;
        self.frame_pool = Some(frame_pool);
        self.session.replace(session);

        Ok(Stream::new(rx))
    }

    fn set_capture_item(&mut self, capture_item: Self::Item) -> Self::Result<()> {
        tracing::info!(
            "Setting capture item: {}",
            capture_item.DisplayName().unwrap_or("<no name>".into())
        );
        self.capture_item = Some(capture_item);
        self.resources.write().unwrap().reset();

        Ok(())
    }

    fn start_capture(&mut self) -> Self::Result<()> {
        if self.capturing {
            tracing::warn!("Tried to start capture, but was already capturing.");
            return Ok(());
        }

        if self.capture_item.is_none() {
            tracing::error!("No capture item set!");
            return Err(Error::NoCaptureItem);
        }

        if let Some(session) = self.session.get() {
            session.StartCapture().map_err(|e| {
                tracing::error!("Failed to start capture! {}", e);
                Error::FailedToStartCapture(e)
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
        crate::media::capture::windows::winrt_to_native_d3d11device(&device)
            .ok()
            .map(crate::media::CodecDeviceLease::from_d3d11)
    }
}

fn same_capture_size(left: SizeInt32, right: SizeInt32) -> bool {
    left.Width == right.Width && left.Height == right.Height
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.stop_capture().ok();
    }
}

unsafe impl Send for Provider {}
unsafe impl Sync for Provider {}

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

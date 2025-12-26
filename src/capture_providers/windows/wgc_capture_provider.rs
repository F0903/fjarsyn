use std::{
    iter::IntoIterator,
    mem::MaybeUninit,
    sync::{Arc, RwLock},
};

use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::Direct3D11::IDirect3DDevice,
    },
    Win32::{
        Graphics::Direct3D11::{
            D3D11_CPU_ACCESS_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Device,
            ID3D11Texture2D,
        },
        System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
    },
};
use windows_core::Interface;

use crate::{
    capture_providers::{
        CaptureProvider,
        windows::{
            WindowsCaptureError, WindowsCaptureStream,
            d3d11_utils::{copy_texture, map_read_texture},
        },
    },
    utils::{
        buffer_arena::{BufferArena, BufferRef},
        frame::Frame,
        pixel_format::PixelFormat,
        vector2::Vector2,
    },
};

#[derive(Debug, Default)]
struct Staging {
    textures: Vec<ID3D11Texture2D>,
    frame_count: u64,
    width: u32,
    height: u32,
}

// Windows Graphics Capture (WGC) Provider
#[derive(Debug)]
pub struct WgcCaptureProvider {
    device: IDirect3DDevice,
    capture_item: Option<GraphicsCaptureItem>,
    pixel_format: PixelFormat,
    staging_state: Arc<RwLock<Staging>>,
    buffer_pool: BufferArena,

    frame_pool: Option<Direct3D11CaptureFramePool>,
    session: Option<GraphicsCaptureSession>,
    stream_tokens: Vec<i64>,
    capturing: bool,
}

impl WgcCaptureProvider {
    const WGC_FRAME_BUFFERS: i32 = 2;
    const PIPELINE_DEPTH: usize = 2;
    const TX_QUEUE_SIZE: usize = 2;
    const BUFFER_ARENA_SIZE: usize = 128000;

    pub fn new(device: IDirect3DDevice, pixel_format: PixelFormat) -> Self {
        Self {
            device,
            capture_item: None,
            pixel_format,
            staging_state: Arc::new(RwLock::new(Staging::default())),
            buffer_pool: BufferArena::init(Self::BUFFER_ARENA_SIZE),
            frame_pool: None,
            session: None,
            stream_tokens: Vec::new(),
            capturing: false,
        }
    }

    fn process_frame(
        mut frame_buffer: BufferRef,
        frame: Direct3D11CaptureFrame,
        staging_state_arc: Arc<RwLock<Staging>>,
        pixel_format: PixelFormat,
        tx: tokio::sync::mpsc::Sender<Frame>,
    ) -> super::Result<()> {
        let surface = frame.Surface().map_err(|e| {
            tracing::error!("Failed to get surface! {}", e);
            WindowsCaptureError::FailedToGetSurface(e)
        })?;

        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| {
            tracing::error!("Failed to cast surface to access! {}", e);
            WindowsCaptureError::CastFailed(e)
        })?;

        let texture: ID3D11Texture2D = unsafe {
            access.GetInterface().map_err(|e| {
                tracing::error!("Failed to get interface! {}", e);
                WindowsCaptureError::FailedToGetInterface(e)
            })?
        };

        let size = frame.ContentSize().map_err(|e| {
            tracing::error!("Failed to get frame ContentSize! {}", e);
            WindowsCaptureError::FailedToGetContentSize(e)
        })?;

        tracing::trace!("Frame: {} x {}, ptr={:?}", size.Width, size.Height, texture.as_raw());

        let device = unsafe {
            texture.GetDevice().map_err(|e| {
                tracing::error!("Failed to get device: {}", e);
                WindowsCaptureError::FailedToGetDevice(e)
            })?
        };

        let context = unsafe {
            device.GetImmediateContext().map_err(|e| {
                tracing::error!("Failed to get immediate context: {}", e);
                WindowsCaptureError::FailedToGetImmediateContext(e)
            })?
        };

        let desc = unsafe {
            let mut d = std::mem::zeroed::<D3D11_TEXTURE2D_DESC>();
            texture.GetDesc(&mut d);
            d.BindFlags = 0;
            d.MiscFlags = 0;
            d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            d.Usage = D3D11_USAGE_STAGING;
            d.MipLevels = 1;
            d.ArraySize = 1;
            d.SampleDesc.Count = 1;
            d.SampleDesc.Quality = 0;
            d
        };

        let mut staging = Self::ensure_staging_state(&device, &staging_state_arc, desc)?;
        let write_idx = (staging.frame_count % Self::PIPELINE_DEPTH as u64) as usize;
        let write_tex = &staging.textures[write_idx];

        // 1. Copy current frame to the current ("write") staging texture (GPU operation, async)
        copy_texture(&context, &texture, write_tex);

        // 2. Read from the previous ("read") staging texture (CPU operation, ideally finished by now)
        let index = (staging.frame_count.wrapping_sub(1)) as usize % Self::PIPELINE_DEPTH;
        let read_tex = &staging.textures[index];
        map_read_texture(
            &mut frame_buffer,
            &context,
            read_tex,
            &desc,
            pixel_format.bytes_per_pixel(),
        )?;

        staging.frame_count += 1;

        let rel_time = frame
            .SystemRelativeTime()
            .map_err(|e| {
                tracing::warn!("Failed to get frame system relative time: {}", e);
                e
            })
            .unwrap_or_default();

        let frame_duration = std::time::Duration::from_nanos((rel_time.Duration / 100) as u64);

        let dirty_regions = match frame.DirtyRegions() {
            Ok(regions) => regions.into_iter().map(Into::into).collect(),
            Err(err) => {
                tracing::warn!("Failed to get frame dirty regions: {}", err);
                Vec::new()
            }
        };

        let frame = Frame::new_ensure_rgba(
            frame_buffer,
            pixel_format,
            Vector2 { x: size.Width, y: size.Height },
            Some(frame_duration),
            Some(dirty_regions),
        );

        match tx.try_send(frame) {
            Ok(_) => (),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!("Frame sender closed whilst trying to send frame.");
                return Err(WindowsCaptureError::FrameSenderClosed);
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!("Frame channel full, dropping frame.");
            }
        }

        Ok(())
    }

    fn ensure_staging_state<'a>(
        device: &'a ID3D11Device,
        staging_state_arc: &'a Arc<RwLock<Staging>>,
        desc: D3D11_TEXTURE2D_DESC,
    ) -> super::Result<std::sync::RwLockWriteGuard<'a, Staging>> {
        let mut staging = staging_state_arc.write().unwrap();

        // Initialize or re-initialize the staging pool if needed
        if staging.textures.is_empty()
            || staging.width != desc.Width
            || staging.height != desc.Height
        {
            tracing::info!(
                "Initializing staging pool with depth {} for size {}x{}",
                Self::TX_QUEUE_SIZE,
                desc.Width,
                desc.Height
            );

            // Clear existing textures since we are possibly resizing
            staging.textures.clear();
            staging.width = desc.Width;
            staging.height = desc.Height;
            staging.frame_count = 0; // Reset pipeline state

            for _ in 0..Self::PIPELINE_DEPTH {
                let staging_tex = unsafe {
                    let mut tex = MaybeUninit::<Option<ID3D11Texture2D>>::uninit();
                    match device.CreateTexture2D(&desc, None, Some(tex.as_mut_ptr())) {
                        Ok(_) => (),
                        Err(err) => {
                            tracing::error!("Failed to create staging texture: {}", err);
                            return Err(WindowsCaptureError::FailedToCreateTexture(err));
                        }
                    }
                    tex.assume_init().expect("Failed to create staging texture!")
                };
                staging.textures.push(staging_tex);
            }
        }

        Ok(staging)
    }
}

impl CaptureProvider for WgcCaptureProvider {
    type Result<T> = super::Result<T>;
    type Stream = WindowsCaptureStream;
    type CaptureItem = GraphicsCaptureItem;

    fn create_stream(
        &mut self,
        framerate: crate::capture_providers::shared::CaptureFramerate,
    ) -> Self::Result<Self::Stream> {
        let (tx, rx) = tokio::sync::mpsc::channel(Self::PIPELINE_DEPTH);

        let capture_item = self.capture_item.as_ref().ok_or_else(|| {
            tracing::error!("No capture item set!");
            WindowsCaptureError::NoCaptureItem
        })?;

        let device = self.device.clone();
        let staging_state_arc = self.staging_state.clone();

        let size = capture_item.Size().map_err(|e| {
            tracing::error!("Failed to get size of capture item! {}", e);
            WindowsCaptureError::FailedToGetCaptureItemSize(e)
        })?;

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device,
            self.pixel_format.to_directx_pixel_format(),
            Self::WGC_FRAME_BUFFERS,
            size,
        )
        .map_err(|e| {
            tracing::error!("Failed to create frame pool! {}", e);
            WindowsCaptureError::FailedToCreateFramePool(e)
        })?;

        let session = frame_pool.CreateCaptureSession(capture_item).map_err(|e| {
            tracing::error!("Failed to create capture session! {}", e);
            WindowsCaptureError::FailedToCreateCaptureSession(e)
        })?;

        if let Err(e) = session.SetIsCursorCaptureEnabled(true) {
            tracing::warn!("Failed to set IsCursorCaptureEnabled: {}", e);
        }
        if let Err(e) = session.SetIsBorderRequired(true) {
            tracing::warn!("Failed to set IsBorderRequired: {}", e);
        }

        session.SetMinUpdateInterval(framerate.to_frametime().into()).map_err(|e| {
            tracing::error!("Failed to set MinUpdateInterval: {}", e);
            WindowsCaptureError::FailedToSetMinUpdateInterval(e)
        })?;

        let buffer_pool = self.buffer_pool.clone();
        let staging_state_arc = staging_state_arc.clone();
        let pixel_format = self.pixel_format.clone();

        let token = frame_pool
            .FrameArrived(&TypedEventHandler::new(move |sender, _| {
                let sender: &Direct3D11CaptureFramePool = match &*sender {
                    Some(s) => s,
                    None => return Ok(()),
                };

                match sender.TryGetNextFrame() {
                    Ok(frame) => {
                        let content_size = frame.ContentSize().unwrap_or(size);
                        let buffer_size = content_size.Width as usize
                            * content_size.Height as usize
                            * pixel_format.bytes_per_pixel() as usize;

                        if buffer_size == 0 {
                            tracing::warn!("Frame content size is 0, skipping frame.");
                            return Ok(());
                        }

                        let mut buffer = buffer_pool.get(buffer_size);
                        unsafe {
                            buffer.set_len(buffer_size);
                        }

                        match Self::process_frame(
                            buffer,
                            frame,
                            staging_state_arc.clone(),
                            pixel_format,
                            tx.clone(),
                        ) {
                            Ok(()) => (),
                            Err(WindowsCaptureError::FrameSenderClosed) => (),
                            Err(e) => {
                                tracing::error!("Failed to process frame: {}", e);
                                // We can't return a custom error here
                            }
                        }
                    }
                    Err(e) => tracing::error!("Failed to get next frame: {}", e),
                }

                Ok(())
            }))
            .map_err(|e| {
                tracing::error!("Failed to set FrameArrived handler! {}", e);
                WindowsCaptureError::FailedToSetFrameArrivedHandler(e)
            })?;
        tracing::debug!("Added frame arrived handler with token: {}", token);
        self.stream_tokens.push(token);

        if self.capturing {
            session.StartCapture().map_err(|e| {
                tracing::error!("Failed to start capture! {}", e);
                WindowsCaptureError::FailedToStartCapture(e)
            })?;
        }

        self.frame_pool = Some(frame_pool);
        self.session = Some(session);

        Ok(WindowsCaptureStream::new(rx))
    }

    fn set_capture_item(&mut self, capture_item: Self::CaptureItem) -> Self::Result<()> {
        tracing::info!(
            "Setting capture item: {}",
            capture_item.DisplayName().unwrap_or("<no name>".into())
        );
        self.capture_item = Some(capture_item);

        // Reset staging state
        {
            let mut state = self.staging_state.write().unwrap();
            state.textures.clear();
            state.frame_count = 0;
        }

        Ok(())
    }

    fn start_capture(&mut self) -> Self::Result<()> {
        if self.capturing {
            tracing::warn!("Tried to start capture, but was already capturing.");
            return Err(WindowsCaptureError::AlreadyCapturing);
        }

        if self.capture_item.is_none() {
            tracing::error!("No capture item set!");
            return Err(WindowsCaptureError::NoCaptureItem);
        }

        if let Some(session) = &self.session {
            session.StartCapture().map_err(|e| {
                tracing::error!("Failed to start capture! {}", e);
                WindowsCaptureError::FailedToStartCapture(e)
            })?;
        }

        self.capturing = true;
        Ok(())
    }

    fn stop_capture(&mut self) -> Self::Result<()> {
        if !self.capturing {
            return Ok(());
        }

        if let Some(session) = &self.session {
            session.Close().ok();
        }
        if let Some(frame_pool) = &self.frame_pool {
            for token in self.stream_tokens.drain(..) {
                tracing::debug!("Removing frame arrived handler: {}", token);
                frame_pool.RemoveFrameArrived(token).ok();
            }
            frame_pool.Close().ok();
        }

        self.session = None;
        self.frame_pool = None;
        self.capturing = false;
        Ok(())
    }

    fn is_capturing(&self) -> bool {
        self.capturing
    }
}

impl Drop for WgcCaptureProvider {
    fn drop(&mut self) {
        self.stop_capture().ok();
    }
}

// WgcCaptureProvider holds agile COM objects that are thread-safe.
unsafe impl Send for WgcCaptureProvider {}
unsafe impl Sync for WgcCaptureProvider {}

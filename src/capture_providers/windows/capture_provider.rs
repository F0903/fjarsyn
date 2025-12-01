use std::{
    mem::MaybeUninit,
    sync::{Arc, Mutex, OnceLock, RwLock},
    thread::{self, JoinHandle},
};

use windows::{
    Foundation::TypedEventHandler,
    Graphics::{Capture::*, DirectX::Direct3D11::*},
    Win32::{
        Foundation::{LPARAM, WPARAM},
        Graphics::Direct3D11::*,
        System::{
            Com::{CO_MTA_USAGE_COOKIE, CoDecrementMTAUsage, CoIncrementMTAUsage},
            Threading::GetCurrentThreadId,
            WinRT::{
                CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
                Direct3D11::IDirect3DDxgiInterfaceAccess, DispatcherQueueOptions,
                RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
            },
        },
        UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, MSG, PostQuitMessage, PostThreadMessageW,
            TranslateMessage, WM_QUIT,
        },
    },
    core::*,
};
use windows_future::AsyncActionCompletedHandler;

use crate::{
    capture_providers::{
        CaptureProvider,
        shared::{
            BytesPerPixel, CaptureFramerate, Frame, PixelFormat, ToDirectXPixelFormat, Vector2,
        },
        windows::{
            WindowsCaptureStream,
            d3d11_utils::{copy_texture, map_read_texture},
            error::WindowsCaptureError,
        },
    },
    utils::UnsafeSendWrapper,
};

#[derive(Debug, Default)]
struct StagingState {
    textures: Vec<ID3D11Texture2D>,
    frame_count: u64,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct CaptureThreadInfo {
    handle: JoinHandle<()>,
    thread_id: u32,
}

#[derive(Debug)]
pub struct WindowsCaptureProvider {
    device: IDirect3DDevice,
    capture_item: Option<GraphicsCaptureItem>,
    staging_state: Arc<RwLock<StagingState>>,

    // Shared state with the capture thread
    // The thread reads this to know where to send frames.
    frame_sender: Arc<Mutex<Option<tokio::sync::mpsc::Sender<Frame>>>>,

    // Thread management
    capture_thread: Option<CaptureThreadInfo>,
    capturing: bool,
}

impl WindowsCaptureProvider {
    const WGC_FRAME_BUFFERS: i32 = 3;
    const PIXEL_FORMAT: PixelFormat = PixelFormat::BGRA8;
    const PIPELINE_DEPTH: usize = 2;
    const TX_QUEUE_SIZE: usize = 2;

    pub fn new(device: IDirect3DDevice, item: Option<GraphicsCaptureItem>) -> Self {
        Self {
            device,
            capture_item: item,
            staging_state: Arc::new(RwLock::new(StagingState::default())),
            frame_sender: Arc::new(Mutex::new(None)),
            capture_thread: None,
            capturing: false,
        }
    }

    /// This function runs on the dedicated capture thread.
    fn capture_loop(
        device: UnsafeSendWrapper<IDirect3DDevice>,
        item: GraphicsCaptureItem,
        framerate: CaptureFramerate,
        staging_state: Arc<RwLock<StagingState>>,
        frame_sender: Arc<Mutex<Option<tokio::sync::mpsc::Sender<Frame>>>>,
        ready_tx: std::sync::mpsc::Sender<super::Result<u32>>,
    ) {
        unsafe {
            static INIT_MTA: OnceLock<UnsafeSendWrapper<CO_MTA_USAGE_COOKIE>> = OnceLock::new();
            INIT_MTA.get_or_init(|| {
                UnsafeSendWrapper(CoIncrementMTAUsage().expect("Failed to increment MTA usage"))
            });

            // Initialize WinRT for this thread
            if let Err(_e) = RoInitialize(RO_INIT_MULTITHREADED) {
                // Ignore S_FALSE (already initialized)
                tracing::debug!("Tried to initialize WinRT, but was already initialized.")
            }

            // Create DispatcherQueue for this thread (Required for WGC events)
            let options = DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            };

            let controller = match CreateDispatcherQueueController(options) {
                Ok(c) => c,
                Err(e) => {
                    ready_tx
                        .send(Err(WindowsCaptureError::FailedToCreateDispatcherQueueController(e)))
                        .ok();
                    return;
                }
            };

            let size = match item.Size() {
                Ok(s) => s,
                Err(e) => {
                    ready_tx.send(Err(WindowsCaptureError::UnknownWindowsError(e))).ok();
                    return;
                }
            };

            // Create FramePool on this thread
            let frame_pool = match Direct3D11CaptureFramePool::Create(
                &device.0,
                Self::PIXEL_FORMAT.to_directx_pixel_format(),
                Self::WGC_FRAME_BUFFERS,
                size,
            ) {
                Ok(fp) => fp,
                Err(e) => {
                    ready_tx.send(Err(WindowsCaptureError::FailedToCreateFramePool(e))).ok();
                    return;
                }
            };

            // Create Session
            let session = match frame_pool.CreateCaptureSession(&item) {
                Ok(s) => s,
                Err(e) => {
                    ready_tx.send(Err(WindowsCaptureError::FailedToCreateCaptureSession(e))).ok();
                    return;
                }
            };
            session.SetIsCursorCaptureEnabled(true).ok();
            session.SetIsBorderRequired(true).ok();
            session.SetMinUpdateInterval(framerate.to_frametime().into()).ok();

            // Set up Frame Arrived Handler
            let staging_state_clone = staging_state.clone();
            let frame_sender_clone = frame_sender.clone();

            let _token = match frame_pool.FrameArrived(&TypedEventHandler::new(move |sender, _| {
                let sender: &Direct3D11CaptureFramePool = match &*sender {
                    Some(s) => s,
                    None => return Ok(()),
                };

                if let Ok(frame) = sender.TryGetNextFrame() {
                    let tx_opt = frame_sender_clone.lock().unwrap().clone();
                    if let Some(tx) = tx_opt {
                        let _ = Self::process_frame(frame, staging_state_clone.clone(), tx);
                    }
                }
                Ok(())
            })) {
                Ok(t) => t,
                Err(_) => {
                    ready_tx.send(Err(WindowsCaptureError::StagingStateLockFailed)).ok(); // Generic error
                    return;
                }
            };

            if let Err(e) = session.StartCapture() {
                ready_tx.send(Err(WindowsCaptureError::FailedToSetMinUpdateInterval(e))).ok();
                return;
            }

            // Signal success to main thread
            let thread_id = GetCurrentThreadId();
            ready_tx.send(Ok(thread_id)).ok();

            // Run Message Loop (pumps DispatcherQueue)
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            // Cleanup
            session.Close().ok();
            frame_pool.Close().ok();

            match controller.ShutdownQueueAsync() {
                Ok(shutdown) => {
                    if let Err(err) = shutdown.SetCompleted(&AsyncActionCompletedHandler::new(
                        move |_, _| -> windows_core::Result<()> {
                            PostQuitMessage(0);
                            Ok(())
                        },
                    )) {
                        tracing::error!(
                            "Failed to set DispatcherQueueController shutdown completion handler: {}",
                            err
                        );
                    }
                }
                Err(err) => {
                    tracing::error!("Failed to shutdown DispatcherQueueController: {}", err);
                }
            };

            if let Some(mta_cookie) = INIT_MTA.get() {
                if let Err(err) = CoDecrementMTAUsage(**mta_cookie) {
                    tracing::error!("Failed to decrement MTA usage: {}", err);
                }
            }

            RoUninitialize();
        }
    }

    fn process_frame(
        frame: Direct3D11CaptureFrame,
        staging_state_arc: Arc<RwLock<StagingState>>,
        tx: tokio::sync::mpsc::Sender<Frame>,
    ) -> super::Result<()> {
        // Direct3D11CaptureFrame → IDirect3DSurface
        let surface = match frame.Surface() {
            Ok(surface) => surface,
            Err(err) => {
                tracing::error!("Failed to get surface: {}", err);
                return Ok(());
            }
        };

        // IDirect3DSurface → IDirect3DDxgiInterfaceAccess
        let access: IDirect3DDxgiInterfaceAccess = match surface.cast() {
            Ok(access) => access,
            Err(err) => {
                tracing::error!("Failed to cast surface to access: {}", err);
                return Ok(());
            }
        };

        // IDirect3DDxgiInterfaceAccess → ID3D11Texture2D
        let texture: ID3D11Texture2D = match unsafe { access.GetInterface() } {
            Ok(texture) => texture,
            Err(err) => {
                tracing::error!("Failed to cast access to texture: {}", err);
                return Ok(());
            }
        };

        let size = match frame.ContentSize() {
            Ok(size) => size,
            Err(err) => {
                tracing::error!("Failed to get content size: {}", err);
                return Ok(());
            }
        };

        tracing::trace!("Frame: {} x {}, ptr={:?}", size.Width, size.Height, texture.as_raw());

        let device = unsafe {
            match texture.GetDevice() {
                Ok(device) => device,
                Err(err) => {
                    tracing::error!("Failed to get device: {}", err);
                    return Ok(());
                }
            }
        };

        let context = unsafe {
            match device.GetImmediateContext() {
                Ok(context) => context,
                Err(err) => {
                    tracing::error!("Failed to get immediate context: {}", err);
                    return Ok(());
                }
            }
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

        let (data, frame_metadata) = {
            let mut staging = match staging_state_arc.write() {
                Ok(state) => state,
                Err(_) => return Err(WindowsCaptureError::StagingStateLockFailed),
            };

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
                                return Ok(());
                            }
                        }
                        tex.assume_init().expect("Failed to create staging texture!")
                    };
                    staging.textures.push(staging_tex);
                }
            }

            let write_idx = (staging.frame_count % Self::PIPELINE_DEPTH as u64) as usize;
            let write_tex = &staging.textures[write_idx];

            // 1. Copy current frame to the "write" staging texture (GPU operation, async)
            copy_texture(&context, &texture, write_tex);

            // 2. Read from the "read" staging texture (CPU operation, ideally finished by now)
            let bytes = if staging.frame_count > 0 {
                let index = (staging.frame_count.wrapping_sub(1)) as usize % Self::PIPELINE_DEPTH;
                let read_tex = &staging.textures[index];
                match map_read_texture(
                    &context,
                    read_tex,
                    &desc,
                    Self::PIXEL_FORMAT.bytes_per_pixel(),
                ) {
                    Ok(b) => Some(b),
                    Err(e) => {
                        tracing::warn!("Failed to map/read texture: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            staging.frame_count += 1;

            let sys_time = match frame.SystemRelativeTime() {
                Ok(time) => time,
                Err(err) => {
                    tracing::error!("Failed to get system relative time: {}", err);
                    return Ok(());
                }
            };

            let dirty_regions = match frame.DirtyRegions() {
                Ok(regions) => regions.into_iter().map(Into::into).collect(),
                Err(err) => {
                    tracing::warn!("Failed to get dirty regions: {}", err);
                    Vec::new()
                }
            };

            (bytes, (sys_time, dirty_regions))
        };

        if let Some(data) = data {
            let (sys_time, dirty_regions) = frame_metadata;
            let frame = Frame::new_ensure_rgba(
                data,
                Self::PIXEL_FORMAT,
                Vector2 { x: size.Width, y: size.Height },
                sys_time.Duration,
                dirty_regions,
            );

            match tx.try_send(frame) {
                Ok(_) => (),
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    tracing::warn!("Frame sender closed whilst trying to send frame.");
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    tracing::debug!("Frame channel full, dropping frame.");
                }
            }
        }

        Ok(())
    }

    // Posts a quit message to the capture thread and waits for it to exit.
    fn ensure_quit_capture_thread(&mut self) {
        if let Some(thread_info) = self.capture_thread.take() {
            tracing::info!("Stopping capture thread...");
            unsafe {
                match PostThreadMessageW(thread_info.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) {
                    Ok(_) => (),
                    Err(err) => {
                        tracing::error!("Failed to post quit message to capture thread: {}", err)
                    }
                }
            }

            if let Err(_) = thread_info.handle.join() {
                tracing::error!("Failed to join capture thread.");
            } else {
                tracing::info!("Capture thread stopped.");
            }
        }
    }
}

impl CaptureProvider for WindowsCaptureProvider {
    type Result<T> = super::Result<T>;
    type Stream = WindowsCaptureStream;
    type CaptureItem = GraphicsCaptureItem;

    /// Creates a new stream for receiving frames.
    fn create_stream(&mut self, framerate: CaptureFramerate) -> Self::Result<Self::Stream> {
        self.ensure_quit_capture_thread();

        // Create new channel
        let (tx, rx) = tokio::sync::mpsc::channel(Self::PIPELINE_DEPTH);

        // Update the sender that the thread uses
        {
            let mut sender_guard = self.frame_sender.lock().unwrap();
            *sender_guard = Some(tx);
        }

        let capture_item = match &self.capture_item {
            Some(item) => item.clone(),
            None => {
                tracing::error!("No capture item set!");
                return Err(WindowsCaptureError::NoCaptureItem);
            }
        };

        let device = UnsafeSendWrapper(self.device.clone());
        let staging_state = self.staging_state.clone();
        let frame_sender = self.frame_sender.clone();

        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        tracing::info!("Spawning capture thread with framerate: {}...", framerate);
        let handle = thread::spawn(move || {
            Self::capture_loop(
                device,
                capture_item,
                framerate,
                staging_state,
                frame_sender,
                ready_tx,
            );
        });

        match ready_rx.recv() {
            Ok(Ok(thread_id)) => {
                tracing::info!("Capture thread started successfully (ID: {}).", thread_id);
                self.capture_thread = Some(CaptureThreadInfo { handle, thread_id });
                Ok(WindowsCaptureStream::new(rx))
            }
            Ok(Err(e)) => {
                tracing::error!("Capture thread failed to start: {}", e);
                handle.join().ok();
                Err(e)
            }
            Err(_) => {
                tracing::error!("Capture thread panicked or disconnected before reporting status.");
                handle.join().ok();
                Err(WindowsCaptureError::NoFramePool)
            }
        }
    }

    fn set_capture_item(&mut self, capture_item: Self::CaptureItem) -> Self::Result<()> {
        tracing::info!(
            "Setting capture item: {}",
            capture_item.DisplayName().unwrap_or("<no name>".into())
        );
        self.capture_item = Some(capture_item);

        // Reset staging state
        {
            let mut state = match self.staging_state.write() {
                Ok(state) => state,
                Err(_) => return Err(WindowsCaptureError::StagingStateLockFailed),
            };
            state.textures.clear();
            state.frame_count = 0;
        }

        Ok(())
    }

    fn start_capture(&mut self) -> Self::Result<()> {
        if self.capturing {
            return Err(WindowsCaptureError::AlreadyCapturing);
        }

        if self.capture_item.is_none() {
            tracing::error!("No capture item set!");
            return Err(WindowsCaptureError::NoCaptureItem);
        }

        self.capturing = true;
        Ok(())
    }

    fn stop_capture(&mut self) -> Self::Result<()> {
        if !self.capturing {
            // It's possible we aren't capturing but the thread is still there (cleanup)
            // But generally follow the flag.
            return Ok(());
        }

        self.ensure_quit_capture_thread();

        // Clear sender
        {
            let mut sender_guard = self.frame_sender.lock().unwrap();
            *sender_guard = None;
        }

        self.capturing = false;
        Ok(())
    }
}

impl Drop for WindowsCaptureProvider {
    fn drop(&mut self) {
        self.stop_capture().ok();
    }
}

// WindowsCaptureProvider holds agile COM objects that are thread-safe.
unsafe impl Send for WindowsCaptureProvider {}
unsafe impl Sync for WindowsCaptureProvider {}

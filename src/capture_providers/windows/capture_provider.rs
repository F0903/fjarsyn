use std::{
    mem::MaybeUninit,
    ops::Div,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::sync::{RwLock, mpsc::error::TryRecvError};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{Capture::*, DirectX::Direct3D11::*},
    Win32::{Graphics::Direct3D11::*, System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess},
    core::*,
};

use crate::capture_providers::{
    shared::{BytesPerPixel, CaptureFramerate, Frame, PixelFormat, ToDirectXPixelFormat, Vector2},
    windows::{WindowsCaptureStream, d3d11_utils::read_texture, error::WindowsCaptureError},
};

#[derive(Debug)]
pub struct WindowsCaptureProvider {
    device: IDirect3DDevice,                        /* Free-threaded object */
    frame_pool: Option<Direct3D11CaptureFramePool>, /* Free-threaded object */
    capture_item: Option<GraphicsCaptureItem>,      /* Free-threaded object */
    session: Option<GraphicsCaptureSession>,        /* Free-threaded object */
    staging_texture: Arc<RwLock<Option<ID3D11Texture2D>>>, /* Free-threaded object */

    stream_close_tx: tokio::sync::mpsc::Sender<i64>,
    stream_close_rx: tokio::sync::mpsc::Receiver<i64>,

    capturing: bool,
}

impl WindowsCaptureProvider {
    const FRAME_COUNT: i32 = 2;
    const PIXEL_FORMAT: PixelFormat = PixelFormat::BGRA8;

    pub fn new(device: IDirect3DDevice, item: Option<GraphicsCaptureItem>) -> super::Result<Self> {
        let (stream_close_tx, stream_close_rx) = tokio::sync::mpsc::channel(32);
        Ok(Self {
            device,
            frame_pool: None,
            capture_item: item,
            session: None,
            staging_texture: Arc::new(RwLock::new(None)),

            stream_close_tx,
            stream_close_rx,

            capturing: false,
        })
    }

    pub fn set_capture_item(&mut self, capture_item: GraphicsCaptureItem) -> super::Result<()> {
        tracing::info!(
            "Setting capture item: {}",
            capture_item.DisplayName().unwrap_or("<no name>".into())
        );

        let size = capture_item.Size()?;
        self.capture_item = Some(capture_item);

        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &self.device,
            Self::PIXEL_FORMAT.to_directx_pixel_format(),
            Self::FRAME_COUNT,
            size,
        )?;
        self.frame_pool = Some(frame_pool);

        // Disabled for testing reasons, uncomment later
        //session.SetIsCursorCaptureEnabled(true)?;
        //session.SetIsBorderRequired(false)?;
        Ok(())
    }

    fn process_frame(
        frame: Direct3D11CaptureFrame,
        staging_tex_arc: Arc<RwLock<Option<ID3D11Texture2D>>>,
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

        tracing::debug!("Frame: {} x {}, ptr={:?}", size.Width, size.Height, texture.as_raw());

        let device = unsafe {
            match texture.GetDevice() {
                Ok(device) => device,
                Err(err) => {
                    tracing::error!("Failed to get device: {}", err);
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

        let staging_tex = { staging_tex_arc.blocking_read().clone() };
        let staging_tex = match staging_tex {
            Some(staging_tex) => staging_tex,
            None => unsafe {
                let mut tex = MaybeUninit::<Option<ID3D11Texture2D>>::uninit();
                match device.CreateTexture2D(&desc, None, Some(tex.as_mut_ptr())) {
                    Ok(_) => (),
                    Err(err) => {
                        tracing::error!("Failed to create staging texture: {}", err);
                        return Ok(());
                    }
                }

                let staging_tex = tex.assume_init().expect("Failed to create staging texture!");
                *staging_tex_arc.blocking_write() = Some(staging_tex.clone());
                staging_tex
            },
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

        let data = read_texture(
            &context,
            texture,
            staging_tex,
            &desc,
            Self::PIXEL_FORMAT.bytes_per_pixel(),
        )
        .expect("Unable to read texture into byte array!");

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
                Vec::new() // Dirty regions are currently not used, and should generally be safe to skip in this case.
            }
        };

        let frame = Frame::new_ensure_rgba(
            data,
            crate::capture_providers::shared::PixelFormat::BGRA8,
            Vector2 { x: size.Width, y: size.Height },
            sys_time.Duration,
            dirty_regions,
        );

        let send_result = tx.blocking_send(frame);
        if let Err(err) = send_result {
            tracing::error!("Could not send frame! {}", err);
        }

        Ok(())
    }

    /// Creates a new stream for receiving frames.
    /// Must be called on a COM thread.
    pub fn create_stream(
        &self,
        framerate: CaptureFramerate,
    ) -> super::Result<WindowsCaptureStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(2);

        // We can't send self raw to the closure, so we need to just copy the staging texture which is inside an Arc Mutex.
        let staging_tex_ptr = self.staging_texture.clone();

        #[cfg(debug_assertions)]
        let frame_counter = Arc::new(AtomicUsize::new(0));
        #[cfg(debug_assertions)]
        let frame_counter_weak = Arc::downgrade(&frame_counter);

        #[cfg(debug_assertions)]
        tokio::spawn(async move {
            let mut last_count = 0;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if let Some(counter) = frame_counter_weak.upgrade() {
                    let current_count = counter.load(Ordering::Relaxed);
                    let fps = current_count.wrapping_sub(last_count);
                    tracing::debug!("Capture stream FPS: {}", fps);
                    last_count = current_count;
                } else {
                    tracing::debug!("Capture stream FPS task exited.");
                    break;
                }
            }
        });

        let frame_pool = match &self.frame_pool {
            Some(frame_pool) => frame_pool,
            None => {
                tracing::error!("No frame pool available!");
                return Err(WindowsCaptureError::NoFramePool);
            }
        };

        if let Err(err) =
            self.session.as_ref().unwrap().SetMinUpdateInterval(windows::Foundation::TimeSpan {
                Duration: framerate.to_frametime().as_nanos().div(100) as i64,
            })
        {
            tracing::error!("Failed to set min update interval: {}", err);
            return Err(WindowsCaptureError::SetMinUpdateIntervalFailed(err));
        }

        let frame_arrived_token =
            frame_pool.FrameArrived(&TypedEventHandler::new(move |sender, _args| {
                #[cfg(debug_assertions)]
                frame_counter.fetch_add(1, Ordering::Relaxed);

                let sender = match &*sender {
                    Some(sender) => sender,
                    None => {
                        tracing::error!("No sender provided with FrameArrived!");
                        return Ok(());
                    }
                };
                let sender: &Direct3D11CaptureFramePool = sender;

                let frame = match sender.TryGetNextFrame() {
                    Ok(frame) => frame,
                    Err(err) => {
                        tracing::error!("Failed to get next frame: {}", err);
                        return Ok(());
                    }
                };

                if let Err(err) = Self::process_frame(frame, staging_tex_ptr.clone(), tx.clone()) {
                    tracing::error!("Failed to process frame: {}", err);
                }

                Ok(())
            }))?;

        let stream =
            WindowsCaptureStream::new(self.stream_close_tx.clone(), rx, frame_arrived_token);

        Ok(stream)
    }

    /// Must be called on a COM thread.
    pub fn start_capture(&mut self) -> super::Result<()> {
        if self.capturing {
            return Err(WindowsCaptureError::AlreadyCapturing);
        }

        let frame_pool = match &self.frame_pool {
            Some(frame_pool) => frame_pool,
            None => {
                tracing::error!("No frame pool set!");
                return Err(WindowsCaptureError::NoFramePool);
            }
        };

        let capture_item = match &self.capture_item {
            Some(capture_item) => capture_item,
            None => {
                tracing::error!("No capture item set!");
                return Err(WindowsCaptureError::NoCaptureItem);
            }
        };

        let session = match &self.session {
            Some(session) => session,
            None => {
                let new_session = frame_pool.CreateCaptureSession(capture_item)?;
                self.session = Some(new_session);
                self.session.as_ref().unwrap()
            }
        };

        session.StartCapture()?;
        self.capturing = true;

        Ok(())
    }

    pub fn stop_capture(&mut self) -> super::Result<()> {
        if !self.capturing {
            return Err(WindowsCaptureError::NotCapturing);
        }

        self.session.take(); // Drop the old session
        self.capturing = false;

        Ok(())
    }

    /// Must be called on a COM thread.
    pub fn poll_stream_closer(&mut self) -> super::Result<()> {
        loop {
            let next = self.stream_close_rx.try_recv();
            match next {
                Ok(token) => {
                    self.unregister_frame_arrived(token)?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        Ok(())
    }

    /// Must be called on a COM thread.
    pub(super) fn unregister_frame_arrived(&self, token: i64) -> super::Result<()> {
        let frame_pool = match &self.frame_pool {
            Some(frame_pool) => frame_pool,
            None => {
                return Ok(());
            }
        };

        frame_pool.RemoveFrameArrived(token)?;
        Ok(())
    }
}

impl Drop for WindowsCaptureProvider {
    fn drop(&mut self) {
        self.stop_capture().ok();
    }
}

// WindowsCaptureProvider holds agile COM objects that are thread-safe. But since they are raw pointers, they are not Send or Sync.
unsafe impl Send for WindowsCaptureProvider {}
unsafe impl Sync for WindowsCaptureProvider {}

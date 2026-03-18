use std::{
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
        CaptureFramerate, CaptureProvider,
        windows::{
            WindowsCaptureError, WindowsCaptureStream,
            d3d11_utils::{copy_texture, map_read_texture, winrt_to_native_d3d11device},
        },
    },
    media::{
        frame::{Frame, GpuFrameResource, GpuImportHandle},
        pixel_format::PixelFormat,
    },
    utils::{
        buffer_pool::{Buffer, BufferPool},
        vector2::Vector2,
    },
};

#[derive(Debug, Default)]
struct ResourcePool {
    shared_textures: Vec<ID3D11Texture2D>,
    shared_handles: Vec<GpuImportHandle>,
    staging_textures: Vec<ID3D11Texture2D>,
    frame_count: u64,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy)]
struct CaptureOptions {
    cpu_readback_enabled: bool,
}

// Windows Graphics Capture (WGC) Provider
#[derive(Debug)]
pub struct WgcCaptureProvider {
    device: IDirect3DDevice,
    capture_item: Option<GraphicsCaptureItem>,
    pixel_format: PixelFormat,
    resource_state: Arc<RwLock<ResourcePool>>,
    capture_options: Arc<RwLock<CaptureOptions>>,
    buffer_pool: BufferPool,

    frame_pool: Option<Direct3D11CaptureFramePool>,
    session: Option<GraphicsCaptureSession>,
    stream_tokens: Vec<windows::Foundation::EventRegistrationToken>,
    capturing: bool,

    record_cursor: bool,
    border_indicator: bool,
}

impl WgcCaptureProvider {
    const WGC_FRAME_BUFFERS: i32 = 5;
    const PIPELINE_DEPTH: usize = 3;
    const BUFFER_SIZE: usize = 16 * 1024 * 1024; // 16MB for 4K support
    const BUFFER_MAX_COUNT: usize = 8;

    pub fn new(
        device: IDirect3DDevice,
        pixel_format: PixelFormat,
        record_cursor: bool,
        border_indicator: bool,
        cpu_readback_enabled: bool,
    ) -> Self {
        Self {
            device,
            capture_item: None,
            pixel_format,
            resource_state: Arc::new(RwLock::new(ResourcePool::default())),
            capture_options: Arc::new(RwLock::new(CaptureOptions { cpu_readback_enabled })),
            buffer_pool: BufferPool::init(Self::BUFFER_SIZE, Self::BUFFER_MAX_COUNT),
            frame_pool: None,
            session: None,
            stream_tokens: Vec::new(),
            capturing: false,
            record_cursor,
            border_indicator,
        }
    }

    pub fn set_cpu_readback_enabled(&mut self, enabled: bool) {
        self.capture_options.write().unwrap().cpu_readback_enabled = enabled;
    }

    fn process_frame(
        mut frame_buffer: Option<Buffer>,
        capture_frame: Direct3D11CaptureFrame,
        resource_state_arc: Arc<RwLock<ResourcePool>>,
        pixel_format: PixelFormat,
        tx: tokio::sync::mpsc::Sender<Frame>,
    ) -> super::Result<()> {
        let surface = capture_frame.Surface().map_err(|e| {
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

        let size = capture_frame.ContentSize().map_err(|e| {
            tracing::error!("Failed to get frame ContentSize! {}", e);
            WindowsCaptureError::FailedToGetContentSize(e)
        })?;

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
            d
        };

        let mut pool =
            Self::ensure_resource_pool(&device, &resource_state_arc, desc, frame_buffer.is_some())?;
        let write_idx = (pool.frame_count % Self::PIPELINE_DEPTH as u64) as usize;

        // 1. GPU-to-GPU copy to the Shared texture (for wgpu/zero-copy)
        let shared_tex = &pool.shared_textures[write_idx];
        copy_texture(&context, &texture, shared_tex);

        // Submit to GPU to ensure shared handle reflects latest pixels
        unsafe { context.Flush() };

        // 2. GPU-to-GPU copy to the Staging texture (for CPU mapping) if requested
        if frame_buffer.is_some() {
            let staging_tex = &pool.staging_textures[write_idx];
            copy_texture(&context, &texture, staging_tex);
        }

        // 3. Read from the previous ("read") pool textures
        let read_idx = (pool.frame_count.wrapping_sub(1)) as usize % Self::PIPELINE_DEPTH;

        let shared_handle = pool.shared_handles[read_idx];

        if let Some(buf) = &mut frame_buffer {
            let read_staging_tex = &pool.staging_textures[read_idx];
            let mut staging_desc = desc;
            staging_desc.Usage = D3D11_USAGE_STAGING;
            staging_desc.BindFlags = 0;
            staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            staging_desc.MiscFlags = 0;

            map_read_texture(
                buf,
                &context,
                read_staging_tex,
                &staging_desc,
                pixel_format.bytes_per_pixel(),
            )?;
        }

        pool.frame_count += 1;

        let rel_time = capture_frame
            .SystemRelativeTime()
            .map_err(|e| {
                tracing::warn!("Failed to get frame system relative time: {}", e);
                e
            })
            .unwrap_or_default();

        let frame_duration = std::time::Duration::from_nanos((rel_time.Duration * 100) as u64);

        let resource_owner: std::sync::Arc<dyn std::any::Any + Send + Sync> =
            std::sync::Arc::new(capture_frame);

        let output_frame = Frame::new_gpu(
            GpuFrameResource::D3D11Texture(texture.clone()),
            Some(shared_handle),
            frame_buffer,
            Some(resource_owner),
            pixel_format,
            Vector2 { x: size.Width, y: size.Height },
            Some(frame_duration),
        );

        match tx.try_send(output_frame) {
            Ok(_) => (),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                return Err(WindowsCaptureError::FrameSenderClosed);
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!("Frame channel full, dropping frame.");
            }
        }

        Ok(())
    }

    fn ensure_resource_pool<'a>(
        device: &'a ID3D11Device,
        pool_arc: &'a Arc<RwLock<ResourcePool>>,
        desc: D3D11_TEXTURE2D_DESC,
        require_staging: bool,
    ) -> super::Result<std::sync::RwLockWriteGuard<'a, ResourcePool>> {
        let mut pool = pool_arc.write().unwrap();

        // Re-initialize pool if size changed or staging is now required but missing
        if pool.shared_textures.is_empty()
            || pool.width != desc.Width
            || pool.height != desc.Height
            || (require_staging && pool.staging_textures.is_empty())
        {
            tracing::info!(
                "Initializing resource pool (Shared: {}, Staging: {}) for size {}x{}",
                true,
                require_staging,
                desc.Width,
                desc.Height
            );

            pool.shared_textures.clear();
            pool.shared_handles.clear();
            pool.staging_textures.clear();
            pool.width = desc.Width;
            pool.height = desc.Height;
            pool.frame_count = 0;

            // Shared Texture Description
            let mut shared_desc = desc;
            shared_desc.Usage = windows::Win32::Graphics::Direct3D11::D3D11_USAGE_DEFAULT;
            shared_desc.BindFlags =
                windows::Win32::Graphics::Direct3D11::D3D11_BIND_SHADER_RESOURCE.0 as u32;
            shared_desc.CPUAccessFlags = 0;
            shared_desc.MiscFlags =
                (windows::Win32::Graphics::Direct3D11::D3D11_RESOURCE_MISC_SHARED.0
                    | windows::Win32::Graphics::Direct3D11::D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0)
                    as u32;

            // Staging Texture Description
            let mut staging_desc = desc;
            staging_desc.Usage = windows::Win32::Graphics::Direct3D11::D3D11_USAGE_STAGING;
            staging_desc.BindFlags = 0;
            staging_desc.CPUAccessFlags =
                windows::Win32::Graphics::Direct3D11::D3D11_CPU_ACCESS_READ.0 as u32;
            staging_desc.MiscFlags = 0;

            for _ in 0..Self::PIPELINE_DEPTH {
                // Create Shared Texture
                let shared_tex: ID3D11Texture2D = unsafe {
                    let mut tex = MaybeUninit::<Option<ID3D11Texture2D>>::uninit();
                    device.CreateTexture2D(&shared_desc, None, Some(tex.as_mut_ptr())).map_err(
                        |err| {
                            tracing::error!("Failed to create shared texture: {}", err);
                            WindowsCaptureError::FailedToCreateTexture(err)
                        },
                    )?;
                    tex.assume_init().expect("Failed to create shared texture!")
                };

                let shared_handle = unsafe {
                    let dxgi_res: windows::Win32::Graphics::Dxgi::IDXGIResource1 =
                        shared_tex.cast().map_err(|e| {
                            tracing::error!("Failed to cast to IDXGIResource1: {}", e);
                            e
                        })?;
                    dxgi_res
                        .CreateSharedHandle(
                            None,
                            windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_READ.0
                                | windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_WRITE.0,
                            None,
                        )
                        .map_err(|e| {
                            tracing::error!("Failed to create shared NT handle: {}", e);
                            e
                        })?
                };

                pool.shared_textures.push(shared_tex);
                pool.shared_handles.push(GpuImportHandle::from_windows_nt_handle(shared_handle));

                // Create Staging Texture (if requested)
                if require_staging {
                    let staging_tex: ID3D11Texture2D = unsafe {
                        let mut tex = MaybeUninit::<Option<ID3D11Texture2D>>::uninit();
                        device
                            .CreateTexture2D(&staging_desc, None, Some(tex.as_mut_ptr()))
                            .map_err(|err| {
                                tracing::error!("Failed to create staging texture: {}", err);
                                WindowsCaptureError::FailedToCreateTexture(err)
                            })?;
                        tex.assume_init().expect("Failed to create staging texture!")
                    };
                    pool.staging_textures.push(staging_tex);
                }
            }
        }

        Ok(pool)
    }
}

impl CaptureProvider for WgcCaptureProvider {
    type Result<T> = super::Result<T>;
    type Stream = WindowsCaptureStream;
    type CaptureItem = GraphicsCaptureItem;

    fn create_stream(&mut self, framerate: CaptureFramerate) -> Self::Result<Self::Stream> {
        // Stop any existing session before starting a new one
        self.stop_capture().ok();

        let (tx, rx) = tokio::sync::mpsc::channel(Self::PIPELINE_DEPTH);

        let capture_item = self.capture_item.as_ref().ok_or_else(|| {
            tracing::error!("No capture item set!");
            WindowsCaptureError::NoCaptureItem
        })?;

        let device = self.device.clone();
        let resource_state_arc = self.resource_state.clone();

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

        if let Err(e) = session.SetIsCursorCaptureEnabled(self.record_cursor) {
            tracing::warn!("Failed to set IsCursorCaptureEnabled: {}", e);
        }
        if let Err(e) = session.SetIsBorderRequired(self.border_indicator) {
            tracing::warn!("Failed to set IsBorderRequired: {}", e);
        }

        session.SetMinUpdateInterval(framerate.to_frametime().into()).map_err(|e| {
            tracing::error!("Failed to set MinUpdateInterval: {}", e);
            WindowsCaptureError::FailedToSetMinUpdateInterval(e)
        })?;

        let buffer_pool = self.buffer_pool.clone();
        let resource_state_arc_inner = resource_state_arc.clone();
        let capture_options = self.capture_options.clone();
        let pixel_format = self.pixel_format;

        let token = frame_pool
            .FrameArrived(&TypedEventHandler::new(move |sender, _| {
                // If the channel is closed, we shouldn't even try to get the frame.
                if tx.is_closed() {
                    return Ok(());
                }

                let sender: &Direct3D11CaptureFramePool = match sender {
                    Some(s) => s,
                    None => return Ok(()),
                };

                match sender.TryGetNextFrame() {
                    Ok(frame) => {
                        let content_size = frame.ContentSize().unwrap_or(size);
                        let mut buffer = None;
                        let capture_options = *capture_options.read().unwrap();

                        // Only allocate CPU memory when downstream consumers genuinely need it.
                        if capture_options.cpu_readback_enabled {
                            let buffer_size = content_size.Width as usize
                                * content_size.Height as usize
                                * pixel_format.bytes_per_pixel() as usize;

                            if buffer_size > 0 {
                                let mut b = buffer_pool.get_unzeroed(buffer_size);
                                unsafe {
                                    b.set_len(buffer_size);
                                }
                                buffer = Some(b);
                            }
                        }

                        match Self::process_frame(
                            buffer,
                            frame,
                            resource_state_arc_inner.clone(),
                            pixel_format,
                            tx.clone(),
                        ) {
                            Ok(()) => (),
                            Err(WindowsCaptureError::FrameSenderClosed) => (),
                            Err(e) => {
                                tracing::error!("Failed to process frame: {}", e);
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
        tracing::debug!("Added frame arrived handler with token: {:?}", token);
        self.stream_tokens.push(token);

        // ALWAYS start capture when a stream is created
        session.StartCapture().map_err(|e| {
            tracing::error!("Failed to start capture! {}", e);
            WindowsCaptureError::FailedToStartCapture(e)
        })?;

        self.capturing = true;

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
            let mut state = self.resource_state.write().unwrap();
            state.shared_textures.clear();
            state.shared_handles.clear();
            state.staging_textures.clear();
            state.frame_count = 0;
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
        // Convert the WinRT device to the native COM ID3D11Device
        winrt_to_native_d3d11device(&self.device).ok().map(|d| {
            // We wrap it in ManuallyDrop because FFmpeg's AVHWDeviceContext takes ownership
            // of this device pointer and will call Release() on it when destroyed.
            let d = std::mem::ManuallyDrop::new(d);
            windows_core::Interface::as_raw(&*d)
        })
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

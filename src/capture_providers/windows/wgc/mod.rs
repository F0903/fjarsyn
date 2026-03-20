use std::sync::{Arc, RwLock};

use windows::Graphics::{
    Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
    DirectX::Direct3D11::IDirect3DDevice,
};

use crate::{media::pixel_format::PixelFormat, utils::buffer_pool::BufferPool};

mod builder;
mod processing;
mod resources;
mod session;

pub use builder::{WgcCaptureProviderBuilder, WgcCaptureProviderBuilderError};

use super::{Result, WindowsCaptureStream};

#[derive(Debug, Default)]
struct ResourcePool {
    shared_textures: Vec<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>,
    shared_handles: Vec<crate::media::frame::GpuImportHandle>,
    staging_textures: Vec<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>,
    frame_count: u64,
    last_emitted_timestamp_100ns: Option<i64>,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy)]
struct CaptureOptions {
    cpu_readback_enabled: bool,
}

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
}

unsafe impl Send for WgcCaptureProvider {}
unsafe impl Sync for WgcCaptureProvider {}

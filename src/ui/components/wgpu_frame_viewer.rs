use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use iced::{Element, Length, Rectangle, mouse, widget::shader};
use iced_wgpu::graphics::Viewport;
use wgpu::hal::api::Dx12;
use windows::Win32::Graphics::Direct3D12 as d3d12;

use crate::media::{
    frame::{Frame, FrameData, SyncHandle},
    pixel_format::PixelFormat,
};

pub struct WgpuFrameViewer {
    frame: Arc<Frame>,
}

impl WgpuFrameViewer {
    pub fn new(frame: Arc<Frame>) -> Self {
        Self { frame }
    }
}

impl<'a, Message: 'a> From<WgpuFrameViewer> for Element<'a, Message, iced::Theme, iced::Renderer> {
    fn from(viewer: WgpuFrameViewer) -> Self {
        shader::Shader::new(viewer).width(Length::Fill).height(Length::Fill).into()
    }
}

impl<Message> shader::Program<Message> for WgpuFrameViewer {
    type State = ();
    type Primitive = Primitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        Primitive { frame: self.frame.clone() }
    }
}

#[derive(Debug)]
pub struct Primitive {
    frame: Arc<Frame>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    ndc_min: [f32; 2],
    ndc_max: [f32; 2],
}

pub struct Pipeline {
    pipeline_bgra8: wgpu::RenderPipeline,
    pipeline_rgba8: wgpu::RenderPipeline,
    pipeline_rgba16f: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    cache: std::sync::Mutex<HashMap<SyncHandle, (wgpu::BindGroup, Instant)>>,
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        // 1. Calculate the transformation using Normalized Device Coordinates (NDC) relative to the widget
        // Iced's `shader` widget automatically sets the wgpu viewport to match the widget's physical bounds.
        // Therefore, NDC [-1.0, 1.0] maps perfectly to the bounds of the widget itself.
        let bounds_w = bounds.width;
        let bounds_h = bounds.height;

        let src_w = self.frame.size.x as f32;
        let src_h = self.frame.size.y as f32;

        let aspect_widget = bounds_w / bounds_h;
        let aspect_image = src_w / src_h;

        let (scale_x, scale_y) = if aspect_image > aspect_widget {
            // Image is wider than widget. Fit to width.
            (1.0, aspect_widget / aspect_image)
        } else {
            // Image is taller than widget. Fit to height.
            (aspect_image / aspect_widget, 1.0)
        };

        // Note: wgpu NDC Y points up.
        // pos[0] is Top-Left (-scale_x, scale_y)
        // pos[1] is Top-Right (scale_x, scale_y)
        // pos[2] is Bottom-Left (-scale_x, -scale_y)
        // pos[3] is Bottom-Right (scale_x, -scale_y)
        let ndc_x = -scale_x;
        let ndc_x_max = scale_x;
        let ndc_y = scale_y; // Top
        let ndc_y_max = -scale_y; // Bottom

        let uniforms = Uniforms { ndc_min: [ndc_x, ndc_y], ndc_max: [ndc_x_max, ndc_y_max] };
        queue.write_buffer(&pipeline.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Diagnostics
        static mut LAST_LOG: Option<Instant> = None;
        unsafe {
            if LAST_LOG.map_or(true, |l| l.elapsed() > Duration::from_secs(5)) {
                tracing::info!(
                    "Preview Layout: Widget={:?}, ScaleX={}, ScaleY={}",
                    bounds,
                    scale_x,
                    scale_y
                );
                LAST_LOG = Some(Instant::now());
            }
        }

        // 2. Resource Caching
        let FrameData::D3D11 { shared_handle, .. } = &self.frame.data else {
            return;
        };

        let Some(sync_handle) = *shared_handle else {
            return;
        };

        let mut cache = pipeline.cache.lock().unwrap();
        if let Some(entry) = cache.get_mut(&sync_handle) {
            entry.1 = Instant::now();
            return;
        }

        let now = Instant::now();
        cache.retain(|_, (_, timestamp)| now.duration_since(*timestamp) < Duration::from_secs(1));

        let handle = sync_handle.0;
        let width = self.frame.size.x as u32;
        let height = self.frame.size.y as u32;

        let format = match self.frame.format {
            PixelFormat::BGRA8 => wgpu::TextureFormat::Bgra8Unorm,
            PixelFormat::RGBA8 => wgpu::TextureFormat::Rgba8Unorm,
            PixelFormat::RGBA16 => wgpu::TextureFormat::Rgba16Float,
            PixelFormat::RGBA10 => wgpu::TextureFormat::Rgb10a2Unorm,
            _ => wgpu::TextureFormat::Bgra8Unorm, // Fallback
        };

        unsafe {
            let Some(hal_device) = _device.as_hal::<Dx12>() else {
                return;
            };

            let raw_device = hal_device.raw_device();
            let mut raw_resource: Option<d3d12::ID3D12Resource> = None;

            if let Err(e) = raw_device.OpenSharedHandle(handle, &mut raw_resource) {
                tracing::error!("Failed to open shared handle: {}", e);
                return;
            }

            let raw_resource = raw_resource.unwrap();
            let hal_texture = wgpu::hal::dx12::Device::texture_from_raw(
                raw_resource,
                format,
                wgpu::TextureDimension::D2,
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                1,
                1,
            );

            let texture_desc = wgpu::TextureDescriptor {
                label: Some("Imported Shared Texture"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            };

            let texture = _device.create_texture_from_hal::<Dx12>(hal_texture, &texture_desc);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            let bind_group = _device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Frame Viewer Bind Group"),
                layout: &pipeline.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&pipeline.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: pipeline.uniform_buffer.as_entire_binding(),
                    },
                ],
            });

            cache.insert(sync_handle, (bind_group, Instant::now()));
        }
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let FrameData::D3D11 { shared_handle, .. } = &self.frame.data else {
            return false;
        };

        let Some(sync_handle) = *shared_handle else {
            return false;
        };

        let render_pipeline = match self.frame.format {
            PixelFormat::BGRA8 => &pipeline.pipeline_bgra8,
            PixelFormat::RGBA8 => &pipeline.pipeline_rgba8,
            PixelFormat::RGBA16 => &pipeline.pipeline_rgba16f,
            _ => &pipeline.pipeline_bgra8,
        };

        let cache = pipeline.cache.lock().unwrap();
        if let Some((bind_group, _)) = cache.get(&sync_handle) {
            render_pass.set_pipeline(render_pipeline);
            render_pass.set_bind_group(0, bind_group, &[]);
            render_pass.draw(0..4, 0..1);
            true
        } else {
            false
        }
    }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, _format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Frame Viewer Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "frame_viewer.wgsl"
            ))),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Frame Viewer Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Frame Viewer Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // We must create a pipeline for the specific surface format Iced is using (usually Bgra8Unorm or Rgba8Unorm)
        // Iced passes the swapchain format in `_format`!
        let create_pipeline = || {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Frame Viewer Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: _format, // MUST match the render pass target (the window surface format)
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };

        let pipeline = create_pipeline();

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Viewer Uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline_bgra8: pipeline.clone(),
            pipeline_rgba8: pipeline.clone(),
            pipeline_rgba16f: pipeline,
            sampler,
            bind_group_layout,
            uniform_buffer,
            cache: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

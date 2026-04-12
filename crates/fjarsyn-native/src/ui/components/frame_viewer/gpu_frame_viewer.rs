use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use fjarsyn_core::media::{
    frame::{Frame, GpuImportHandle},
    gpu_interop::{self, ImportedFrameTexture},
};
use iced::{Element, Length, Rectangle, mouse, widget::shader};
use iced_wgpu::graphics::Viewport;

pub struct GpuFrameViewer {
    frame: Arc<Frame>,
}

impl GpuFrameViewer {
    pub fn new(frame: Arc<Frame>) -> Self {
        Self { frame }
    }
}

impl<'a, Message: 'a> From<GpuFrameViewer> for Element<'a, Message, iced::Theme, iced::Renderer> {
    fn from(viewer: GpuFrameViewer) -> Self {
        shader::Shader::new(viewer).width(Length::Fill).height(Length::Fill).into()
    }
}

impl<Message> shader::Program<Message> for GpuFrameViewer {
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
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    cache: std::sync::Mutex<HashMap<GpuImportHandle, CachedFrameTexture>>,
}

struct CachedFrameTexture {
    bind_group: wgpu::BindGroup,
    _texture: ImportedFrameTexture,
    last_used: Instant,
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
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

        let ndc_x = -scale_x;
        let ndc_x_max = scale_x;
        let ndc_y = scale_y; // Top
        let ndc_y_max = -scale_y; // Bottom

        let uniforms = Uniforms { ndc_min: [ndc_x, ndc_y], ndc_max: [ndc_x_max, ndc_y_max] };
        queue.write_buffer(&pipeline.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let Some(import_handle) = self.frame.gpu_import_handle() else {
            return;
        };

        let mut cache = pipeline.cache.lock().unwrap();
        if let Some(entry) = cache.get_mut(&import_handle) {
            entry.last_used = Instant::now();
            return;
        }

        let now = Instant::now();
        cache.retain(|_, entry| now.duration_since(entry.last_used) < Duration::from_secs(1));

        let Some(texture) = gpu_interop::import_frame_texture(device, &self.frame) else {
            return;
        };

        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
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

        cache.insert(
            import_handle,
            CachedFrameTexture { bind_group, _texture: texture, last_used: now },
        );
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(import_handle) = self.frame.gpu_import_handle() else {
            return false;
        };

        let cache = pipeline.cache.lock().unwrap();
        if let Some(entry) = cache.get(&import_handle) {
            render_pass.set_pipeline(&pipeline.pipeline);
            render_pass.set_bind_group(0, &entry.bind_group, &[]);
            render_pass.draw(0..4, 0..1);
            true
        } else {
            false
        }
    }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                    format,
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
        });

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
            pipeline,
            sampler,
            bind_group_layout,
            uniform_buffer,
            cache: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

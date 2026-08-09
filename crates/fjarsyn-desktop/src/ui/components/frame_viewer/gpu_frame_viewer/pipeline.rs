use std::time::Instant;

use fjarsyn_engine::media::frame::Frame;
use iced::{Rectangle, widget::shader};

use super::{completion_pump::CompletionPump, frame_cache::FrameCache};

pub(super) struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    frames: FrameCache,
    queue: wgpu::Queue,
    completion_pump: CompletionPump,
}

impl Pipeline {
    pub(super) fn prepare_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        frame: &Frame,
    ) {
        self.frames.prepare(device, queue, &self.bind_group_layout, &self.sampler, bounds, frame);
    }

    pub(super) fn draw_frame(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        frame: &Frame,
        bounds: &Rectangle,
    ) -> bool {
        let Some(prepared) = self.frames.prepared_view(frame, bounds) else {
            return false;
        };
        let bind_group = match prepared.prepare_draw(&self.queue, frame) {
            Ok(Some(guard)) => {
                // Register before recording the draw. If recording unwinds and
                // the encoder is discarded, wgpu discards this callback too and
                // the guard safely releases a draw that was never submitted.
                self.completion_pump.retain_until_submitted_work_done(render_pass, guard);
                prepared.bind_group
            }
            Ok(None) => prepared.bind_group,
            Err(error) => {
                self.frames.report_draw_failure(frame, &error, Instant::now());
                let Some(failure_bind_group) = prepared.failure_bind_group else {
                    tracing::error!("An imported frame has no synchronization-failure placeholder");
                    return false;
                };
                failure_bind_group
            }
        };

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..4, 0..1);
        true
    }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
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

        Self {
            pipeline,
            sampler,
            bind_group_layout,
            frames: FrameCache::new(device, queue),
            queue: queue.clone(),
            completion_pump: CompletionPump::new(device),
        }
    }

    fn trim(&mut self) {
        self.frames.trim();
    }
}

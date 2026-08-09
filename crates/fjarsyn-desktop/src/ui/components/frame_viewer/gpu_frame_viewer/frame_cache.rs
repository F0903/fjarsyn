use std::{collections::HashMap, sync::Arc, time::Instant};

use fjarsyn_engine::media::{
    PixelFormat,
    frame::{Frame, GpuResourceId},
    gpu_interop::{self, ImportedFrameTexture},
};
use iced::Rectangle;

use super::uniforms::{UniformKey, Uniforms};

const IMPORT_FAILURE_LOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

pub(super) struct FrameCache {
    sources: HashMap<GpuResourceId, CachedSource>,
    views: HashMap<PreparedViewId, CachedView>,
    placeholder: Arc<FrameTexture>,
    last_import_failure_log: Option<Instant>,
}

impl FrameCache {
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            sources: HashMap::new(),
            views: HashMap::new(),
            placeholder: Arc::new(FrameTexture::Owned(placeholder_texture(device, queue))),
            last_import_failure_log: None,
        }
    }

    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        bounds: &Rectangle,
        frame: &Frame,
    ) {
        let Some(resource_id) = frame.gpu_resource_id() else {
            return;
        };
        let Some(uniforms) = Uniforms::for_frame(bounds, frame) else {
            tracing::warn!(?resource_id, "Cannot prepare a GPU frame with invalid dimensions");
            return;
        };

        let now = Instant::now();
        let view_id = PreparedViewId { resource: resource_id, uniforms: uniforms.key() };
        if let Some(view) = self.views.get_mut(&view_id) {
            view.used_this_frame = true;
            if let Some(source) = self.sources.get_mut(&resource_id) {
                source.used_this_frame = true;
            }
            return;
        }

        let source = self.source_for(device, queue, frame, resource_id, now);
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Viewer Uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let texture_view = source.create_view();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Frame Viewer Bind Group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry { binding: 2, resource: uniform_buffer.as_entire_binding() },
            ],
        });

        self.views.insert(
            view_id,
            CachedView {
                bind_group,
                _uniform_buffer: uniform_buffer,
                _source: source,
                used_this_frame: true,
            },
        );
    }

    pub(super) fn bind_group(&self, frame: &Frame, bounds: &Rectangle) -> Option<&wgpu::BindGroup> {
        let resource = frame.gpu_resource_id()?;
        let uniforms = Uniforms::for_frame(bounds, frame)?;
        let view_id = PreparedViewId { resource, uniforms: uniforms.key() };
        self.views.get(&view_id).map(|view| &view.bind_group)
    }

    fn source_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        resource_id: GpuResourceId,
        now: Instant,
    ) -> Arc<FrameTexture> {
        if let Some(source) = self.sources.get_mut(&resource_id) {
            source.used_this_frame = true;
            return source.texture.clone();
        }

        let texture = match gpu_interop::import_frame_texture(device, queue, frame) {
            Ok(import) => {
                debug_assert_eq!(import.resource_id(), resource_id);
                Arc::new(FrameTexture::Imported(import))
            }
            Err(error) => match upload_software_frame(device, queue, frame) {
                Some(texture) => {
                    if self.should_report_import_failure(now) {
                        tracing::warn!(
                            ?resource_id,
                            %error,
                            "GPU frame import failed; using retained CPU pixels"
                        );
                    }
                    Arc::new(FrameTexture::Owned(texture))
                }
                None => {
                    if self.should_report_import_failure(now) {
                        tracing::error!(
                            ?resource_id,
                            %error,
                            "GPU frame import failed and no CPU fallback is available"
                        );
                    }
                    self.placeholder.clone()
                }
            },
        };

        self.sources
            .insert(resource_id, CachedSource { texture: texture.clone(), used_this_frame: true });
        texture
    }

    fn should_report_import_failure(&mut self, now: Instant) -> bool {
        if self
            .last_import_failure_log
            .is_some_and(|last| now.duration_since(last) < IMPORT_FAILURE_LOG_INTERVAL)
        {
            return false;
        }

        self.last_import_failure_log = Some(now);
        true
    }

    pub(super) fn trim(&mut self) {
        self.sources.retain(|_, source| source.used_this_frame);
        self.views.retain(|_, view| view.used_this_frame);

        for source in self.sources.values_mut() {
            source.used_this_frame = false;
        }
        for view in self.views.values_mut() {
            view.used_this_frame = false;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PreparedViewId {
    resource: GpuResourceId,
    uniforms: UniformKey,
}

struct CachedSource {
    texture: Arc<FrameTexture>,
    used_this_frame: bool,
}

struct CachedView {
    bind_group: wgpu::BindGroup,
    _uniform_buffer: wgpu::Buffer,
    _source: Arc<FrameTexture>,
    used_this_frame: bool,
}

enum FrameTexture {
    Imported(ImportedFrameTexture),
    Owned(wgpu::Texture),
}

impl FrameTexture {
    fn create_view(&self) -> wgpu::TextureView {
        match self {
            Self::Imported(texture) => texture.create_view(),
            Self::Owned(texture) => texture.create_view(&wgpu::TextureViewDescriptor::default()),
        }
    }
}

fn upload_software_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    frame: &Frame,
) -> Option<wgpu::Texture> {
    let pixels = frame.software_pixels()?;
    let (width, height, bytes_per_row, format) =
        software_upload_layout(frame.format, frame.size.width, frame.size.height, pixels.len())?;

    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Frame Viewer CPU Fallback"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(height),
        },
        size,
    );

    Some(texture)
}

fn software_upload_layout(
    format: PixelFormat,
    width: i32,
    height: i32,
    pixel_len: usize,
) -> Option<(u32, u32, u32, wgpu::TextureFormat)> {
    let width = u32::try_from(width).ok().filter(|width| *width > 0)?;
    let height = u32::try_from(height).ok().filter(|height| *height > 0)?;
    let format = match format {
        PixelFormat::RGBA8 => wgpu::TextureFormat::Rgba8Unorm,
        PixelFormat::BGRA8 => wgpu::TextureFormat::Bgra8Unorm,
        PixelFormat::RGBA10 | PixelFormat::RGBA16 | PixelFormat::NV12 => return None,
    };
    let bytes_per_row = width.checked_mul(4)?;
    let expected_len = usize::try_from(bytes_per_row.checked_mul(height)?).ok()?;
    (pixel_len == expected_len).then_some((width, height, bytes_per_row, format))
}

fn placeholder_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let size = wgpu::Extent3d { width: 2, height: 2, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Frame Viewer Unavailable Placeholder"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[180, 35, 130, 255, 30, 30, 36, 255, 30, 30, 36, 255, 180, 35, 130, 255],
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8), rows_per_image: Some(2) },
        size,
    );
    texture
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_fallback_requires_an_exact_supported_layout() {
        assert!(software_upload_layout(PixelFormat::RGBA8, 2, 2, 16).is_some());
        assert!(software_upload_layout(PixelFormat::BGRA8, 2, 2, 16).is_some());
        assert!(software_upload_layout(PixelFormat::RGBA8, 2, 2, 15).is_none());
        assert!(software_upload_layout(PixelFormat::NV12, 2, 2, 8).is_none());
    }
}

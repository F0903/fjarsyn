use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use fjarsyn_engine::media::{
    PixelFormat,
    frame::{Frame, GpuFrameId, GpuTextureId},
    gpu_interop::{self, ImportError, ImportedFrameDrawGuard, ImportedFrameTexture},
};
use iced::Rectangle;

use super::uniforms::{UniformKey, Uniforms};

const IMPORT_FAILURE_LOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
const MAX_CACHED_IMPORTS: usize = 32;

pub(super) struct FrameCache {
    imports: HashMap<GpuTextureId, CachedImport>,
    fallbacks: HashMap<GpuFrameId, CachedFallback>,
    views: HashMap<PreparedViewId, CachedView>,
    placeholder: Arc<FrameTexture>,
    last_import_failure_log: Option<Instant>,
    last_draw_failure_log: Mutex<Option<Instant>>,
    cache_epoch: u64,
}

impl FrameCache {
    pub(super) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            imports: HashMap::new(),
            fallbacks: HashMap::new(),
            views: HashMap::new(),
            placeholder: Arc::new(FrameTexture::Owned(placeholder_texture(device, queue))),
            last_import_failure_log: None,
            last_draw_failure_log: Mutex::new(None),
            cache_epoch: 0,
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
        let Some(frame_id) = frame.gpu_frame_id() else {
            return;
        };
        let Some(texture_id) = frame.gpu_texture_id() else {
            return;
        };
        let Some(uniforms) = Uniforms::for_frame(bounds, frame) else {
            tracing::warn!(?frame_id, "Cannot prepare a GPU frame with invalid dimensions");
            return;
        };

        let now = Instant::now();
        let source = self.source_for(device, queue, frame, frame_id, texture_id, now);
        let view_id =
            PreparedViewId { source: source.identity(frame_id), uniforms: uniforms.key() };
        if let Some(view) = self.views.get_mut(&view_id) {
            view.used_this_frame = true;
            return;
        }

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Frame Viewer Uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let bind_group = bind_group_for(
            device,
            bind_group_layout,
            sampler,
            &uniform_buffer,
            &source,
            "Frame Viewer Bind Group",
        );
        let failure_bind_group = source.is_imported().then(|| {
            bind_group_for(
                device,
                bind_group_layout,
                sampler,
                &uniform_buffer,
                &self.placeholder,
                "Frame Viewer Synchronization Failure Bind Group",
            )
        });

        self.views.insert(
            view_id,
            CachedView {
                bind_group,
                failure_bind_group,
                _uniform_buffer: uniform_buffer,
                _source: source,
                used_this_frame: true,
            },
        );
    }

    pub(super) fn prepared_view(
        &self,
        frame: &Frame,
        bounds: &Rectangle,
    ) -> Option<PreparedView<'_>> {
        let frame_id = frame.gpu_frame_id()?;
        let texture_id = frame.gpu_texture_id()?;
        let uniforms = Uniforms::for_frame(bounds, frame)?;
        let source = if self.imports.contains_key(&texture_id) {
            SourceIdentity::Imported(texture_id)
        } else if self.fallbacks.contains_key(&frame_id) {
            SourceIdentity::Fallback(frame_id)
        } else {
            return None;
        };
        let view_id = PreparedViewId { source, uniforms: uniforms.key() };
        self.views.get(&view_id).map(|view| PreparedView {
            bind_group: &view.bind_group,
            failure_bind_group: view.failure_bind_group.as_ref(),
            source: &view._source,
        })
    }

    fn source_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        frame_id: GpuFrameId,
        texture_id: GpuTextureId,
        now: Instant,
    ) -> Arc<FrameTexture> {
        if let Some(source) = self.imports.get_mut(&texture_id) {
            source.used_this_frame = true;
            source.last_used_epoch = self.cache_epoch;
            return source.texture.clone();
        }
        if let Some(source) = self.fallbacks.get_mut(&frame_id) {
            source.used_this_frame = true;
            return source.texture.clone();
        }

        match gpu_interop::import_frame_texture(device, frame) {
            Ok(import) if import.texture_id() == texture_id => {
                let texture = Arc::new(FrameTexture::Imported(import));
                self.imports.insert(
                    texture_id,
                    CachedImport {
                        texture: texture.clone(),
                        used_this_frame: true,
                        last_used_epoch: self.cache_epoch,
                    },
                );
                texture
            }
            Ok(import) => {
                let imported_texture_id = import.texture_id();
                tracing::error!(
                    ?frame_id,
                    ?texture_id,
                    ?imported_texture_id,
                    "GPU frame import returned a different physical texture; using fallback"
                );
                self.fallback_for(device, queue, frame, frame_id, None, now)
            }
            Err(error) => self.fallback_for(device, queue, frame, frame_id, Some(&error), now),
        }
    }

    fn fallback_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        frame_id: GpuFrameId,
        import_error: Option<&ImportError>,
        now: Instant,
    ) -> Arc<FrameTexture> {
        let texture = match upload_software_frame(device, queue, frame) {
            Some(texture) => {
                if self.should_report_import_failure(now) {
                    if let Some(error) = import_error {
                        tracing::warn!(
                            ?frame_id,
                            %error,
                            "GPU frame import failed; using retained CPU pixels"
                        );
                    } else {
                        tracing::warn!(
                            ?frame_id,
                            "GPU frame import was inconsistent; using retained CPU pixels"
                        );
                    }
                }
                Arc::new(FrameTexture::Owned(texture))
            }
            None => {
                if self.should_report_import_failure(now) {
                    if let Some(error) = import_error {
                        tracing::error!(
                            ?frame_id,
                            %error,
                            "GPU frame import failed and no CPU fallback is available"
                        );
                    } else {
                        tracing::error!(
                            ?frame_id,
                            "GPU frame import was inconsistent and no CPU fallback is available"
                        );
                    }
                }
                self.placeholder.clone()
            }
        };

        self.fallbacks
            .insert(frame_id, CachedFallback { texture: texture.clone(), used_this_frame: true });
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

    pub(super) fn report_draw_failure(&self, frame: &Frame, error: &ImportError, now: Instant) {
        let mut last_log =
            self.last_draw_failure_log.lock().unwrap_or_else(|error| error.into_inner());
        if last_log.is_some_and(|last| now.duration_since(last) < IMPORT_FAILURE_LOG_INTERVAL) {
            return;
        }
        *last_log = Some(now);

        tracing::error!(
            frame_id = ?frame.gpu_frame_id(),
            texture_id = ?frame.gpu_texture_id(),
            %error,
            "GPU frame synchronization failed; drawing the unavailable placeholder"
        );
    }

    pub(super) fn trim(&mut self) {
        self.fallbacks.retain(|_, source| source.used_this_frame);
        self.views.retain(|_, view| view.used_this_frame);

        if self.imports.len() > MAX_CACHED_IMPORTS {
            let mut eviction_candidates = self
                .imports
                .iter()
                .filter(|(_, source)| !source.used_this_frame)
                .map(|(texture_id, source)| (*texture_id, source.last_used_epoch))
                .collect::<Vec<_>>();
            oldest_imports_first(&mut eviction_candidates);

            let eviction_count =
                import_eviction_count(self.imports.len(), eviction_candidates.len());
            for (texture_id, _) in eviction_candidates.into_iter().take(eviction_count) {
                self.imports.remove(&texture_id);
            }
        }

        self.imports.values_mut().for_each(|source| source.used_this_frame = false);
        self.fallbacks.values_mut().for_each(|source| source.used_this_frame = false);
        for view in self.views.values_mut() {
            view.used_this_frame = false;
        }
        self.cache_epoch = self.cache_epoch.checked_add(1).expect("frame cache epoch exhausted");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PreparedViewId {
    source: SourceIdentity,
    uniforms: UniformKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SourceIdentity {
    Imported(GpuTextureId),
    Fallback(GpuFrameId),
}

struct CachedImport {
    texture: Arc<FrameTexture>,
    used_this_frame: bool,
    last_used_epoch: u64,
}

fn oldest_imports_first<K>(candidates: &mut [(K, u64)]) {
    candidates.sort_unstable_by_key(|(_, last_used_epoch)| *last_used_epoch);
}

fn import_eviction_count(total: usize, inactive: usize) -> usize {
    total.saturating_sub(MAX_CACHED_IMPORTS).min(inactive)
}

struct CachedFallback {
    texture: Arc<FrameTexture>,
    used_this_frame: bool,
}

struct CachedView {
    bind_group: wgpu::BindGroup,
    failure_bind_group: Option<wgpu::BindGroup>,
    _uniform_buffer: wgpu::Buffer,
    _source: Arc<FrameTexture>,
    used_this_frame: bool,
}

pub(super) struct PreparedView<'a> {
    pub(super) bind_group: &'a wgpu::BindGroup,
    pub(super) failure_bind_group: Option<&'a wgpu::BindGroup>,
    source: &'a FrameTexture,
}

impl PreparedView<'_> {
    pub(super) fn prepare_draw(
        &self,
        queue: &wgpu::Queue,
        frame: &Frame,
    ) -> Result<Option<ImportedFrameDrawGuard>, ImportError> {
        self.source.prepare_draw(queue, frame)
    }
}

enum FrameTexture {
    Imported(ImportedFrameTexture),
    Owned(wgpu::Texture),
}

impl FrameTexture {
    fn identity(&self, frame_id: GpuFrameId) -> SourceIdentity {
        match self {
            Self::Imported(texture) => SourceIdentity::Imported(texture.texture_id()),
            Self::Owned(_) => SourceIdentity::Fallback(frame_id),
        }
    }

    fn is_imported(&self) -> bool {
        matches!(self, Self::Imported(_))
    }

    fn create_view(&self) -> wgpu::TextureView {
        match self {
            // SAFETY: Imported views are never exposed outside CachedView.
            // Every draw through PreparedView calls prepare_draw with its
            // exact Frame and installs the returned guard on that command
            // buffer's completion callback before recording the draw.
            Self::Imported(texture) => unsafe { texture.create_view() },
            Self::Owned(texture) => texture.create_view(&wgpu::TextureViewDescriptor::default()),
        }
    }

    fn prepare_draw(
        &self,
        queue: &wgpu::Queue,
        frame: &Frame,
    ) -> Result<Option<ImportedFrameDrawGuard>, ImportError> {
        match self {
            Self::Imported(texture) => texture.prepare_draw(queue, frame).map(Some),
            Self::Owned(_) => Ok(None),
        }
    }
}

fn bind_group_for(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    uniform_buffer: &wgpu::Buffer,
    texture: &FrameTexture,
    label: &'static str,
) -> wgpu::BindGroup {
    let texture_view = texture.create_view();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: uniform_buffer.as_entire_binding() },
        ],
    })
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

    #[test]
    fn imported_texture_eviction_is_oldest_first_and_only_under_pressure() {
        let mut candidates = [("newest", 9), ("oldest", 2), ("middle", 5)];
        oldest_imports_first(&mut candidates);

        assert_eq!(candidates.map(|(name, _)| name), ["oldest", "middle", "newest"]);
        assert_eq!(import_eviction_count(MAX_CACHED_IMPORTS, 3), 0);
        assert_eq!(import_eviction_count(MAX_CACHED_IMPORTS + 2, 3), 2);
        assert_eq!(import_eviction_count(MAX_CACHED_IMPORTS + 3, 2), 2);
    }
}

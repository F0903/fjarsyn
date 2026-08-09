#[cfg(target_os = "windows")]
mod pool;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "windows")]
pub(crate) use windows::{D3d11FrameProducer, D3d11FrameWriter};

/// Maximum number of native textures retained by one GPU-frame producer.
///
/// Six slots cover the producer, encoder queue/worker, latest retained state,
/// and a small number of submitted desktop draws without allowing native GPU
/// memory to grow with downstream latency.
pub(crate) const GPU_FRAME_POOL_CAPACITY: usize = 6;

static NEXT_FRAME_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TEXTURE_ID: AtomicU64 = AtomicU64::new(1);

/// Stable process-local identity for one published GPU frame.
///
/// A frame ID identifies content, even when its underlying pooled texture is
/// later reused for different content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuFrameId(u64);

impl GpuFrameId {
    fn next() -> Self {
        let value = NEXT_FRAME_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| current.checked_add(1))
            .expect("GPU frame identity space exhausted");
        Self(value)
    }

    #[cfg(test)]
    const fn from_value(value: u64) -> Self {
        Self(value)
    }
}

/// Stable process-local identity for one pooled GPU texture allocation.
///
/// The same texture ID may back several non-overlapping frame IDs over time.
/// Native handle values are never used as identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuTextureId(u64);

impl GpuTextureId {
    fn next() -> Self {
        let value = NEXT_TEXTURE_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| current.checked_add(1))
            .expect("GPU texture identity space exhausted");
        Self(value)
    }

    #[cfg(test)]
    const fn from_value(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub(crate) struct GpuResource {
    frame_id: GpuFrameId,
    texture_id: GpuTextureId,
    #[cfg(target_os = "windows")]
    windows: windows::Resource,
}

impl GpuResource {
    pub(crate) const fn frame_id(&self) -> GpuFrameId {
        self.frame_id
    }

    pub(crate) const fn texture_id(&self) -> GpuTextureId {
        self.texture_id
    }

    #[cfg(target_os = "windows")]
    pub(crate) const fn windows(&self) -> &windows::Resource {
        &self.windows
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{GpuFrameId, GpuTextureId};

    #[test]
    fn frame_identity_distinguishes_published_content() {
        let first = GpuFrameId::from_value(7);
        let second = GpuFrameId::from_value(8);

        assert_ne!(first, second);
    }

    #[test]
    fn texture_identity_is_a_stable_cache_key() {
        let first = GpuTextureId::from_value(7);
        let second = GpuTextureId::from_value(8);
        let mut cache = HashMap::from([(first, "first"), (second, "second")]);

        assert_eq!(cache.get(&first), Some(&"first"));
        assert_eq!(cache.remove(&GpuTextureId::from_value(8)), Some("second"));
        assert_eq!(cache.len(), 1);
    }
}

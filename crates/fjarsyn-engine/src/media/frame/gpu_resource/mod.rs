#[cfg(target_os = "windows")]
mod windows;

use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "windows")]
pub(crate) use windows::{D3d11FrameProducer, D3d11FrameWriter};

static NEXT_RESOURCE_ID: AtomicU64 = AtomicU64::new(1);

/// Stable process-local identity for one immutable GPU frame resource.
///
/// The identity remains unique across producer and native-device rebuilds. It
/// is opaque so native handle values never become cache keys or ownership
/// tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuResourceId(u64);

impl GpuResourceId {
    fn next() -> Self {
        let value = NEXT_RESOURCE_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| current.checked_add(1))
            .expect("GPU resource identity space exhausted");
        Self(value)
    }

    #[cfg(test)]
    const fn from_value(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub(crate) struct GpuResource {
    id: GpuResourceId,
    #[cfg(target_os = "windows")]
    windows: windows::Resource,
}

impl GpuResource {
    pub(crate) const fn id(&self) -> GpuResourceId {
        self.id
    }

    #[cfg(target_os = "windows")]
    pub(crate) const fn windows(&self) -> &windows::Resource {
        &self.windows
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::GpuResourceId;

    #[test]
    fn resource_identity_distinguishes_every_immutable_resource() {
        let first = GpuResourceId::from_value(7);
        let second = GpuResourceId::from_value(8);

        assert_ne!(first, second);
    }

    #[test]
    fn resource_identity_is_a_stable_cache_key() {
        let first = GpuResourceId::from_value(7);
        let second = GpuResourceId::from_value(8);
        let mut cache = HashMap::from([(first, "first"), (second, "second")]);

        assert_eq!(cache.get(&first), Some(&"first"));
        assert_eq!(cache.remove(&GpuResourceId::from_value(8)), Some("second"));
        assert_eq!(cache.len(), 1);
    }
}

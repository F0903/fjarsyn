use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_RUNTIME_ID: AtomicU64 = AtomicU64::new(1);

/// Process-local identity for one desktop engine runtime.
///
/// The numeric representation stays private so asynchronous boundaries can
/// compare runtime instances without assigning meaning to their values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::ui) struct RuntimeId(u64);

impl RuntimeId {
    pub(in crate::ui) fn next() -> Self {
        let value = NEXT_RUNTIME_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| current.checked_add(1))
            .expect("desktop runtime identity space is exhausted");
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeId;

    #[test]
    fn generated_runtime_ids_are_monotonic() {
        let first = RuntimeId::next();
        let second = RuntimeId::next();

        assert!(second > first);
    }
}

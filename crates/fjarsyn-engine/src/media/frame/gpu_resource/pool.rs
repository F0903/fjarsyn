use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

const AVAILABLE: u8 = 0;
const ACQUIRED: u8 = 1;
const QUARANTINED: u8 = 2;

/// Single-owner checkout over a fixed number of reusable values.
///
/// The pool itself is mutated only by its producer. A lease may move to any
/// consumer thread and makes its slot available again when the lease drops.
#[derive(Debug)]
pub(super) struct Pool<T> {
    capacity: usize,
    slots: Vec<Arc<Slot<T>>>,
}

impl<T> Pool<T> {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "a reusable pool must contain at least one slot");
        Self { capacity, slots: Vec::with_capacity(capacity) }
    }

    /// Acquires a matching idle slot, replaces an idle incompatible slot, or
    /// allocates one new slot while capacity remains.
    ///
    /// The creator is not called when all slots are leased. Creation failure
    /// leaves any existing idle slot untouched.
    pub(super) fn try_acquire<E>(
        &mut self,
        matches: impl Fn(&T) -> bool,
        create: impl FnOnce() -> Result<T, E>,
    ) -> Result<Option<Lease<T>>, E> {
        for slot in &self.slots {
            if matches(slot.value()) && slot.try_acquire() {
                return Ok(Some(Lease::new(slot.clone())));
            }
        }

        let replace = self.slots.iter().position(|slot| slot.is_available());
        if replace.is_none() && self.slots.len() == self.capacity {
            return Ok(None);
        }

        let slot = Arc::new(Slot::acquired(create()?));
        if let Some(index) = replace {
            self.slots[index] = slot.clone();
        } else {
            self.slots.push(slot.clone());
        }
        Ok(Some(Lease::new(slot)))
    }
}

#[derive(Debug)]
struct Slot<T> {
    value: T,
    state: AtomicU8,
}

impl<T> Slot<T> {
    fn acquired(value: T) -> Self {
        Self { value, state: AtomicU8::new(ACQUIRED) }
    }

    fn value(&self) -> &T {
        &self.value
    }

    fn try_acquire(&self) -> bool {
        self.state
            .compare_exchange(AVAILABLE, ACQUIRED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn is_available(&self) -> bool {
        self.state.load(Ordering::Acquire) == AVAILABLE
    }

    fn release(&self) {
        let result =
            self.state.compare_exchange(ACQUIRED, AVAILABLE, Ordering::AcqRel, Ordering::Acquire);
        debug_assert_eq!(result, Ok(ACQUIRED), "reusable pool slot was released illegally");
    }

    fn quarantine(&self) {
        let result =
            self.state.compare_exchange(ACQUIRED, QUARANTINED, Ordering::AcqRel, Ordering::Acquire);
        debug_assert_eq!(result, Ok(ACQUIRED), "reusable pool slot was quarantined illegally");
    }
}

#[derive(Debug)]
pub(super) struct Lease<T> {
    slot: Option<Arc<Slot<T>>>,
}

impl<T> Lease<T> {
    fn new(slot: Arc<Slot<T>>) -> Self {
        Self { slot: Some(slot) }
    }

    pub(super) fn value(&self) -> &T {
        self.slot.as_ref().expect("pool lease slot is present").value()
    }

    /// Permanently removes this slot from circulation until the owning pool is
    /// dropped. This is used when publication synchronization cannot be
    /// established and reuse therefore cannot be proven safe.
    pub(super) fn quarantine(mut self) {
        if let Some(slot) = self.slot.take() {
            slot.quarantine();
        }
    }
}

impl<T> Drop for Lease<T> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            slot.release();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[derive(Debug)]
    struct Value {
        kind: u8,
        serial: usize,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for Value {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn value(kind: u8, serial: usize, drops: &Arc<AtomicUsize>) -> Value {
        Value { kind, serial, drops: drops.clone() }
    }

    #[test]
    fn acquisition_is_bounded_and_nonblocking() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut pool = Pool::with_capacity(2);
        let first = pool
            .try_acquire(|value: &Value| value.kind == 1, || Ok::<_, ()>(value(1, 1, &drops)))
            .unwrap()
            .unwrap();
        let second = pool
            .try_acquire(|value: &Value| value.kind == 1, || Ok::<_, ()>(value(1, 2, &drops)))
            .unwrap()
            .unwrap();

        let exhausted = pool
            .try_acquire(
                |value: &Value| value.kind == 1,
                || -> Result<Value, ()> { panic!("an exhausted pool must not allocate") },
            )
            .unwrap();
        assert!(exhausted.is_none());

        let returned_serial = first.value().serial;
        drop(first);
        let reused = pool
            .try_acquire(
                |value: &Value| value.kind == 1,
                || -> Result<Value, ()> { panic!("a matching returned slot must be reused") },
            )
            .unwrap()
            .unwrap();
        assert_eq!(reused.value().serial, returned_serial);

        drop((reused, second, pool));
        assert_eq!(drops.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn an_idle_incompatible_slot_is_replaced_without_growing() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut pool = Pool::with_capacity(1);
        let first = pool
            .try_acquire(|value: &Value| value.kind == 1, || Ok::<_, ()>(value(1, 1, &drops)))
            .unwrap()
            .unwrap();
        drop(first);

        let replacement = pool
            .try_acquire(|value: &Value| value.kind == 2, || Ok::<_, ()>(value(2, 2, &drops)))
            .unwrap()
            .unwrap();
        assert_eq!(replacement.value().serial, 2);
        assert_eq!(drops.load(Ordering::Relaxed), 1);

        assert!(
            pool.try_acquire(
                |_: &Value| true,
                || -> Result<Value, ()> { panic!("the replacement still occupies the only slot") }
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn creation_failure_preserves_the_idle_slot() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut pool = Pool::with_capacity(1);
        let first = pool
            .try_acquire(|value: &Value| value.kind == 1, || Ok::<_, ()>(value(1, 1, &drops)))
            .unwrap()
            .unwrap();
        drop(first);

        let failed = pool
            .try_acquire(|value: &Value| value.kind == 2, || Err::<Value, _>("allocation failed"));
        assert_eq!(failed.unwrap_err(), "allocation failed");

        let original = pool
            .try_acquire(
                |value: &Value| value.kind == 1,
                || -> Result<Value, &str> {
                    panic!("the original idle slot must survive a failed replacement")
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(original.value().serial, 1);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_lease_keeps_its_value_alive_after_the_pool_is_dropped() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut pool = Pool::with_capacity(1);
        let lease = pool
            .try_acquire(|_: &Value| true, || Ok::<_, ()>(value(1, 1, &drops)))
            .unwrap()
            .unwrap();

        drop(pool);
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        assert_eq!(lease.value().serial, 1);

        drop(lease);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_quarantined_slot_cannot_be_reused() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut pool = Pool::with_capacity(1);
        let lease = pool
            .try_acquire(|_: &Value| true, || Ok::<_, ()>(value(1, 1, &drops)))
            .unwrap()
            .unwrap();

        lease.quarantine();
        assert!(
            pool.try_acquire(
                |_: &Value| true,
                || -> Result<Value, ()> {
                    panic!("a quarantined slot must still consume pool capacity")
                }
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(drops.load(Ordering::Relaxed), 0);

        drop(pool);
        assert_eq!(drops.load(Ordering::Relaxed), 1);
    }
}

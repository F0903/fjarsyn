use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, RwLock},
};

use bytes::BytesMut;

pub type BufferSize = usize;

/// A buffer "arena" based on BytesMut and its ability to split off chunks and reclaim them later to avoid allocations.
///
/// This acts as an arena allocator, as it holds a large contiguous memory block from which it hands out smaller buffers.
/// When you request a buffer, it splits a chunk off the main block.
///
/// Note that previous buffers that are able to be immediately unsplit will be as-is, meaning that buffers might contain garbage data.
///
/// Internally, this wraps a BytesMut insinde an Arc<RwLock>>, so it can be trivially cloned and shared between threads.
#[derive(Debug, Clone)]
pub struct BufferArena {
    arena: Arc<RwLock<BytesMut>>,
}

impl BufferArena {
    pub fn init(capacity: usize) -> Self {
        BufferArena { arena: Arc::new(RwLock::new(BytesMut::with_capacity(capacity))) }
    }

    pub fn get(&self, size: BufferSize) -> BufferRef {
        let mut arena = self.arena.write().unwrap();

        let current_cap = arena.capacity();
        if current_cap < size {
            tracing::debug!(
                "BufferArena was too small, reallocating... old_cap: {}, requested_size: {}",
                current_cap,
                size
            );
            arena.reserve(size);
        }

        let chunk = arena.split_to(size);

        BufferRef::new(chunk, Arc::downgrade(&self.arena))
    }
}

/// A thin wrapper around a BytesMut.
/// This allows the buffer to be immediately unsplit back into the arena when dropped. (if not consumed)
#[derive(Debug)]
pub struct BufferRef {
    data: BytesMut,
    data_taken: bool,
    parent_buffer: std::sync::Weak<RwLock<BytesMut>>,
}

impl BufferRef {
    fn new(data: BytesMut, parent_buffer: std::sync::Weak<RwLock<BytesMut>>) -> Self {
        BufferRef { data, data_taken: false, parent_buffer }
    }

    /// Freezes the underlying buffer into a `Bytes` object.
    /// This is zero-copy and makes the memory immutable.
    pub fn freeze(mut self) -> bytes::Bytes {
        let data = std::mem::take(&mut self.data);
        self.data_taken = true;
        data.freeze()
    }
}

impl Deref for BufferRef {
    type Target = BytesMut;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for BufferRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Drop for BufferRef {
    fn drop(&mut self) {
        if let Some(parent) = self.parent_buffer.upgrade()
            && !self.data_taken
        {
            let mut parent = parent.write().unwrap();
            let data = std::mem::take(&mut self.data);
            parent.unsplit(data); // This shouldn't degenerate into realloc
        }
    }
}

// Allow direct conversion
impl Into<bytes::Bytes> for BufferRef {
    fn into(self) -> bytes::Bytes {
        self.freeze()
    }
}

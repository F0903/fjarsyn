use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, Weak},
};

use bytes::BytesMut;

/// A thread-safe handle to a pool of `BytesMut` buffers to avoid allocations.
#[derive(Debug, Clone)]
pub struct BufferPool {
    data: Arc<Mutex<BufferPoolData>>,
}

impl BufferPool {
    pub fn init(default_capacity: usize, max_buffers: usize) -> Self {
        BufferPool {
            data: Arc::new(Mutex::new(BufferPoolData::init(default_capacity, max_buffers))),
        }
    }

    // Gets a buffer that has capacity at least as large as `size`.
    // The buffer WILL NOT be zeroed out before being returned.
    // This means the returned buffer may contain garbage data.
    pub fn get_unzeroed(&self, size: usize) -> BufferRef {
        let mut pool = self.data.lock().unwrap();
        let default_capacity = pool.default_capacity;

        let data = if let Some(idx) = pool.buffers.iter().position(|b| b.capacity() >= size) {
            let mut buffer = pool.buffers.remove(idx).unwrap();
            unsafe { buffer.set_len(size) };
            buffer
        } else {
            let capacity = size.max(default_capacity);
            let mut buffer = BytesMut::with_capacity(capacity);
            unsafe { buffer.set_len(size) };
            buffer
        };

        BufferRef::new(data, Arc::downgrade(&self.data))
    }
}

#[derive(Debug)]
struct BufferPoolData {
    buffers: VecDeque<BytesMut>,
    default_capacity: usize,
    max_buffers: usize,
}

impl BufferPoolData {
    pub fn init(default_capacity: usize, max_buffers: usize) -> Self {
        Self { buffers: VecDeque::with_capacity(max_buffers), default_capacity, max_buffers }
    }

    fn return_buffer(&mut self, buffer: BytesMut) {
        if self.buffers.len() < self.max_buffers {
            self.buffers.push_back(buffer);
        }
    }
}

/// A thin wrapper around a `BytesMut`.
/// This allows the buffer to be returned to the pool when dropped.
#[derive(Debug)]
pub struct BufferRef {
    data: BytesMut,
    parent_pool: Weak<Mutex<BufferPoolData>>,
}

impl BufferRef {
    fn new(data: BytesMut, parent_pool: Weak<Mutex<BufferPoolData>>) -> Self {
        BufferRef { data, parent_pool }
    }

    /// Freezes the underlying buffer into a `Bytes` object.
    /// This is zero-copy and makes the memory immutable.
    /// The memory will be returned to the pool when the returned `Bytes` (and all its clones) are dropped.
    pub fn freeze(self) -> bytes::Bytes {
        bytes::Bytes::from_owner(self)
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

impl AsRef<[u8]> for BufferRef {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Drop for BufferRef {
    fn drop(&mut self) {
        if let Some(pool) = self.parent_pool.upgrade() {
            let buffer = std::mem::take(&mut self.data);

            // Only return if it actually has capacity (wasn't already taken or empty)
            if buffer.capacity() > 0 {
                let mut pool = pool.lock().unwrap();
                pool.return_buffer(buffer);
            }
        }
    }
}

// Allow direct conversion
impl From<BufferRef> for bytes::Bytes {
    fn from(val: BufferRef) -> Self {
        val.freeze()
    }
}

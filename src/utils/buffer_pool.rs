use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use crossbeam::queue::ArrayQueue;

pub type BufferSize = usize;

// A buffer pool for managing reusable memory buffers.
#[derive(Debug, Clone)]
pub struct BufferPool {
    buffers: Arc<ArrayQueue<Vec<u8>>>,
}

impl BufferPool {
    pub fn init(max_buffers: usize) -> Self {
        BufferPool { buffers: Arc::new(ArrayQueue::new(max_buffers)) }
    }

    pub fn get_or_create(&self, size: BufferSize) -> PooledBuffer {
        let mut vec = self.buffers.pop().unwrap_or_else(|| Vec::with_capacity(size));

        // Ensure it's big enough (in case requested size changed)
        if vec.capacity() < size {
            vec.clear();
            vec.reserve(size - vec.len());
        }

        PooledBuffer { data: vec, buffers: Arc::downgrade(&self.buffers) }
    }
}

// A pooled buffer that holds a weak reference to a buffer in the pool.
#[derive(Debug)]
pub struct PooledBuffer {
    data: Vec<u8>,
    buffers: std::sync::Weak<ArrayQueue<Vec<u8>>>,
}

impl PooledBuffer {
    pub fn new(data: Vec<u8>, buffers: std::sync::Weak<ArrayQueue<Vec<u8>>>) -> Self {
        Self { data, buffers }
    }

    pub fn take(mut self) -> Vec<u8> {
        let vec = std::mem::take(&mut self.data);
        vec
    }
}

impl Deref for PooledBuffer {
    type Target = Vec<u8>;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        // Efficiently swaps 'self.data' with an empty Vec and gives us the original.
        // Vec::new() is free (no allocation), so this is very cheap.
        let vec = std::mem::take(&mut self.data);

        // Return to pool. If pool is full, the vec is just dropped (deallocated).
        if let Some(buffers) = self.buffers.upgrade() {
            buffers.push(vec).ok();
        }
    }
}

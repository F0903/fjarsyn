use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, Weak},
};

use bytes::BytesMut;

#[derive(Debug, Clone)]
pub struct BufferPool {
    data: Arc<Mutex<BufferPoolData>>,
}

impl BufferPool {
    pub fn init(default_capacity: usize, max_buffers: usize) -> Self {
        Self { data: Arc::new(Mutex::new(BufferPoolData::init(default_capacity, max_buffers))) }
    }

    pub fn get_unzeroed(&self, size: usize) -> Buffer {
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

        Buffer::new(data, Arc::downgrade(&self.data))
    }
}

#[derive(Debug)]
struct BufferPoolData {
    buffers: VecDeque<BytesMut>,
    default_capacity: usize,
    max_buffers: usize,
}

impl BufferPoolData {
    fn init(default_capacity: usize, max_buffers: usize) -> Self {
        Self { buffers: VecDeque::with_capacity(max_buffers), default_capacity, max_buffers }
    }

    fn return_buffer(&mut self, buffer: BytesMut) {
        if self.buffers.len() < self.max_buffers {
            self.buffers.push_back(buffer);
        }
    }
}

#[derive(Debug)]
pub struct Buffer {
    data: BytesMut,
    parent_pool: Weak<Mutex<BufferPoolData>>,
}

impl Buffer {
    fn new(data: BytesMut, parent_pool: Weak<Mutex<BufferPoolData>>) -> Self {
        Self { data, parent_pool }
    }

    pub fn freeze(self) -> bytes::Bytes {
        bytes::Bytes::from_owner(self)
    }
}

impl Deref for Buffer {
    type Target = BytesMut;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        if let Some(pool) = self.parent_pool.upgrade() {
            let buffer = std::mem::take(&mut self.data);
            if buffer.capacity() > 0 {
                let mut pool = pool.lock().unwrap();
                pool.return_buffer(buffer);
            }
        }
    }
}

impl From<Buffer> for bytes::Bytes {
    fn from(val: Buffer) -> Self {
        val.freeze()
    }
}

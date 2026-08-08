use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use bytes::BytesMut;

use super::Buffer;

#[derive(Debug)]
pub(super) struct PoolData {
    pub(super) buffers: VecDeque<BytesMut>,
    pub(super) default_capacity: usize,
    max_buffers: usize,
}

impl PoolData {
    fn new(default_capacity: usize, max_buffers: usize) -> Self {
        Self { buffers: VecDeque::with_capacity(max_buffers), default_capacity, max_buffers }
    }

    pub(super) fn return_buffer(&mut self, buffer: BytesMut) {
        if self.buffers.len() < self.max_buffers {
            self.buffers.push_back(buffer);
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Pool {
    data: Arc<Mutex<PoolData>>,
}

impl Pool {
    pub(crate) fn new(default_capacity: usize, max_buffers: usize) -> Self {
        Self { data: Arc::new(Mutex::new(PoolData::new(default_capacity, max_buffers))) }
    }

    pub(crate) fn get(&self, size: usize) -> Buffer {
        let mut pool = self.data.lock().unwrap();
        let default_capacity = pool.default_capacity;

        let data =
            if let Some(index) = pool.buffers.iter().position(|buffer| buffer.capacity() >= size) {
                let mut buffer = pool.buffers.remove(index).unwrap();
                buffer.resize(size, 0);
                buffer
            } else {
                let capacity = size.max(default_capacity);
                let mut buffer = BytesMut::with_capacity(capacity);
                buffer.resize(size, 0);
                buffer
            };

        Buffer::new(data, Arc::downgrade(&self.data))
    }
}

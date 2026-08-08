use std::{
    ops::{Deref, DerefMut},
    sync::{Mutex, Weak},
};

use bytes::BytesMut;

use super::pool::PoolData;

#[derive(Debug)]
pub(crate) struct Buffer {
    data: BytesMut,
    parent_pool: Weak<Mutex<PoolData>>,
}

impl Buffer {
    pub(super) fn new(data: BytesMut, parent_pool: Weak<Mutex<PoolData>>) -> Self {
        Self { data, parent_pool }
    }

    pub(crate) fn freeze(self) -> bytes::Bytes {
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
                pool.lock().unwrap().return_buffer(buffer);
            }
        }
    }
}

impl From<Buffer> for bytes::Bytes {
    fn from(buffer: Buffer) -> Self {
        buffer.freeze()
    }
}

use std::time::Duration;

use crate::{
    capture_providers::shared::{PixelFormat, Rect, Vector2},
    utils::{bitmap_utils::ensure_rgba, buffer_pool::PooledBuffer},
};

#[allow(dead_code)]
#[derive(Debug)]
pub struct Frame {
    pub data: PooledBuffer,
    pub format: PixelFormat,
    pub size: Vector2<i32>,
    pub duration: Duration,
    pub dirty_rects: Vec<Rect<i32>>,
}

impl Frame {
    pub fn new_ensure_rgba(
        mut data: PooledBuffer,
        mut format: PixelFormat,
        size: Vector2<i32>,
        duration: Duration,
        dirty_rects: Vec<Rect<i32>>,
    ) -> Self {
        ensure_rgba(&mut data, &mut format);
        Self::new(data, format, size, duration, dirty_rects)
    }

    fn new(
        data: PooledBuffer,
        format: PixelFormat,
        size: Vector2<i32>,
        duration: Duration,
        dirty_rects: Vec<Rect<i32>>,
    ) -> Self {
        Frame { data, format, size, duration, dirty_rects }
    }
}

use std::time::Duration;

use crate::utils::{
    bitmap_utils::ensure_rgba, buffer_pool::BufferRef, pixel_format::PixelFormat, vector2::Vector2,
};

#[derive(Debug)]
pub struct Frame {
    pub data: BufferRef,
    pub format: PixelFormat,
    pub size: Vector2<i32>,
    pub duration: Option<Duration>,
}

impl Frame {
    pub fn new_ensure_rgba(
        mut data: BufferRef,
        mut format: PixelFormat,
        size: Vector2<i32>,
        duration: Option<Duration>,
    ) -> Self {
        ensure_rgba(&mut data, &mut format);
        Self::new_raw(data, format, size, duration)
    }

    pub fn new_raw(
        data: BufferRef,
        format: PixelFormat,
        size: Vector2<i32>,
        duration: Option<Duration>,
    ) -> Self {
        Frame { data, format, size, duration }
    }
}

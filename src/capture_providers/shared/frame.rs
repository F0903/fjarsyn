use std::time::Duration;

use crate::{
    capture_providers::shared::{PixelFormat, Rect, Vector2},
    utils::{bitmap_utils::ensure_rgba, buffer_arena::BufferRef},
};

#[allow(dead_code)]
#[derive(Debug)]
pub struct Frame {
    pub data: BufferRef,
    pub format: PixelFormat,
    pub size: Vector2<i32>,
    pub duration: Option<Duration>,
    pub dirty_rects: Option<Vec<Rect<i32>>>,
}

impl Frame {
    pub fn new_ensure_rgba(
        mut data: BufferRef,
        mut format: PixelFormat,
        size: Vector2<i32>,
        duration: Option<Duration>,
        dirty_rects: Option<Vec<Rect<i32>>>,
    ) -> Self {
        ensure_rgba(&mut data, &mut format);
        Self::new_raw(data, format, size, duration, dirty_rects)
    }

    pub fn new_raw(
        data: BufferRef,
        format: PixelFormat,
        size: Vector2<i32>,
        duration: Option<Duration>,
        dirty_rects: Option<Vec<Rect<i32>>>,
    ) -> Self {
        Frame { data, format, size, duration, dirty_rects }
    }
}

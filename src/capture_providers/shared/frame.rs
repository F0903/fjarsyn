use bytes::Bytes;

use crate::{
    capture_providers::shared::{PixelFormat, Rect, Vector2},
    utils::image_utils::ensure_image_rgba,
};

#[derive(Debug, Clone)]
pub struct Frame {
    pub data: Bytes,
    pub format: PixelFormat,
    pub size: Vector2<i32>,
    pub timestamp: i64,
    pub dirty_rects: Vec<Rect<i32>>,
}

impl Frame {
    pub fn new_ensure_rgba(
        mut data: Vec<u8>,
        mut format: PixelFormat,
        size: Vector2<i32>,
        timestamp: i64,
        dirty_rects: Vec<Rect<i32>>,
    ) -> Self {
        ensure_image_rgba(&mut data[..], &mut format);
        Self::new(data.into(), format, size, timestamp, dirty_rects)
    }

    fn new(
        data: Bytes,
        format: PixelFormat,
        size: Vector2<i32>,
        timestamp: i64,
        dirty_rects: Vec<Rect<i32>>,
    ) -> Self {
        Frame { data, format, size, timestamp, dirty_rects }
    }
}

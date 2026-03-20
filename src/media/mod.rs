use std::fmt::Display;

use crate::utils::vector2::Vector2;

pub mod bitmap;
pub mod ffmpeg;
pub mod frame;
pub mod gpu_interop;
pub mod pixel_format;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum TargetResolution {
    Scale(Vector2),
    Source,
}

impl TargetResolution {
    pub const ALL: &'static [Self] = &[
        Self::Source,
        Self::Scale(Vector2 { x: 3840, y: 2160 }),
        Self::Scale(Vector2 { x: 1920, y: 1080 }),
        Self::Scale(Vector2 { x: 1280, y: 720 }),
        Self::Scale(Vector2 { x: 854, y: 480 }),
    ];
}

impl Display for TargetResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source => write!(f, "Source"),
            Self::Scale(v) => write!(f, "{}x{}", v.x, v.y),
        }
    }
}

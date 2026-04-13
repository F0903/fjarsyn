use std::{fmt::Display, time::Duration};

use crate::utils::vector2::Vector2;

#[derive(
    Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum CaptureFramerate {
    FPS5,
    FPS24,
    FPS30,
    FPS60,
    FPS120,
    FPS144,
    FPS200,
}

impl CaptureFramerate {
    pub const ALL: &'static [Self] = &[
        Self::FPS5,
        Self::FPS24,
        Self::FPS30,
        Self::FPS60,
        Self::FPS120,
        Self::FPS144,
        Self::FPS200,
    ];

    pub const fn to_hz(&self) -> f32 {
        match self {
            Self::FPS5 => 5.0,
            Self::FPS24 => 24.0,
            Self::FPS30 => 30.0,
            Self::FPS60 => 60.0,
            Self::FPS120 => 120.0,
            Self::FPS144 => 144.0,
            Self::FPS200 => 200.0,
        }
    }

    pub fn to_frametime(&self) -> Duration {
        Duration::from_secs_f32(1.0 / self.to_hz())
    }
}

impl Display for CaptureFramerate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::FPS5 => "5",
            Self::FPS24 => "24",
            Self::FPS30 => "30",
            Self::FPS60 => "60",
            Self::FPS120 => "120",
            Self::FPS144 => "144",
            Self::FPS200 => "200",
        };
        f.write_str(value)
    }
}

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

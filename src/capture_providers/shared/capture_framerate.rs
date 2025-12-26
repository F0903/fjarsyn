use std::{fmt::Display, time::Duration};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Serialize, Deserialize)]
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
    pub const ALL: &[CaptureFramerate] = &[
        CaptureFramerate::FPS5,
        CaptureFramerate::FPS24,
        CaptureFramerate::FPS30,
        CaptureFramerate::FPS60,
        CaptureFramerate::FPS120,
        CaptureFramerate::FPS144,
        CaptureFramerate::FPS200,
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
        match self {
            Self::FPS5 => f.write_str("5"),
            Self::FPS24 => f.write_str("24"),
            Self::FPS30 => f.write_str("30"),
            Self::FPS60 => f.write_str("60"),
            Self::FPS120 => f.write_str("120"),
            Self::FPS144 => f.write_str("144"),
            Self::FPS200 => f.write_str("200"),
        }
    }
}

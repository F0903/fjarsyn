use std::fmt::Display;

use crate::media::Dimensions;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum TargetResolution {
    Scale(Dimensions),
    Source,
}

impl TargetResolution {
    pub const ALL: &'static [Self] = &[
        Self::Source,
        Self::Scale(Dimensions { width: 3840, height: 2160 }),
        Self::Scale(Dimensions { width: 1920, height: 1080 }),
        Self::Scale(Dimensions { width: 1280, height: 720 }),
        Self::Scale(Dimensions { width: 854, height: 480 }),
    ];
}

impl Display for TargetResolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Source => write!(f, "Source"),
            Self::Scale(v) => write!(f, "{}x{}", v.width, v.height),
        }
    }
}

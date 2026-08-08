use std::{fmt::Display, time::Duration};

#[derive(
    Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Framerate {
    FPS5,
    FPS24,
    FPS30,
    FPS60,
    FPS120,
    FPS144,
    FPS200,
}

impl Framerate {
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

impl Display for Framerate {
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

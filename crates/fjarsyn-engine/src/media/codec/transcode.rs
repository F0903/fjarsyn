//! Persisted video-codec selection shared by codec clients and backends.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum TranscodeType {
    #[default]
    H264Software,
    H264Nvenc,
}

impl TranscodeType {
    pub const ALL: &'static [Self] = &[Self::H264Software, Self::H264Nvenc];

    pub const fn uses_hardware_encoder(self) -> bool {
        matches!(self, Self::H264Nvenc)
    }
}

impl std::fmt::Display for TranscodeType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::H264Software => "H.264 (Software)",
            Self::H264Nvenc => "H.264 (NVIDIA)",
        };

        formatter.write_str(label)
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HWAccelType {
    None,
    D3D11VA,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FFmpegTranscodeType {
    H264Software,
    H264Nvenc,
}

impl Default for FFmpegTranscodeType {
    fn default() -> Self {
        Self::H264Software
    }
}

impl FFmpegTranscodeType {
    pub const ALL: &'static [Self] = &[Self::H264Software, Self::H264Nvenc];
}

impl std::fmt::Display for FFmpegTranscodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::H264Software => "H.264 (Software)",
            Self::H264Nvenc => "H.264 (NVIDIA)",
        };

        f.write_str(label)
    }
}

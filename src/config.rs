use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    capture_providers::CaptureFramerate,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType, pixel_format::PixelFormat},
    utils::paths::CONFIG_DIR,
};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Read(#[source] io::Error),
    #[error("Failed to parse config file: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Failed to save default config: {0}")]
    Save(#[source] io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub peer_id: Option<String>,
    pub target_bitrate: u32,
    pub target_framerate: CaptureFramerate,
    pub target_resolution: TargetResolution,
    pub pixel_format: PixelFormat,
    pub max_depacket_latency: u16,
    pub transcoding_type: FFmpegTranscodeType,
    pub record_cursor: bool,
    pub recording_border_indicator: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            peer_id: None,
            target_bitrate: 8_000_000,
            target_framerate: CaptureFramerate::FPS60,
            target_resolution: TargetResolution::Source,
            pixel_format: PixelFormat::BGRA8,
            max_depacket_latency: 1000,
            transcoding_type: FFmpegTranscodeType::default(),
            record_cursor: true,
            recording_border_indicator: true,
        }
    }
}

impl Config {
    fn get_config_path() -> PathBuf {
        CONFIG_DIR.join("config.json")
    }

    pub fn load() -> Result<Self, ConfigError> {
        tracing::info!("Loading config");
        let path = Self::get_config_path();
        if path.exists() {
            let content = fs::read(&path).map_err(ConfigError::Read)?;
            let config: Config = serde_json::from_slice(&content)?;
            return Ok(config);
        }

        tracing::info!("No config file found, creating default config.");
        let default = Self::default();
        default.save().map_err(ConfigError::Save)?;
        Ok(default)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::get_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;

        Ok(())
    }
}

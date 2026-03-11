use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    capture_providers::CaptureFramerate,
    media::{TargetResolution, ffmpeg::FFmpegTranscodeType},
    utils::{paths::CONFIG_DIR, pixel_format::PixelFormat},
};

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
            pixel_format: PixelFormat::RGBA8,
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

    pub fn load() -> Self {
        tracing::info!("Loading config");
        let path = Self::get_config_path();
        if path.exists() {
            match fs::read(&path) {
                Ok(content) => match serde_json::from_slice(&content) {
                    Ok(config) => return config,
                    Err(e) => tracing::error!("Failed to parse config file: {}", e),
                },
                Err(e) => tracing::error!("Failed to read config file: {}", e),
            }
        }

        tracing::info!("No config file could be loaded, using default config.");
        let default = Self::default();
        if let Err(e) = default.save() {
            tracing::error!("Failed to save default config: {}", e);
        }
        default
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

use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    transcoding::FFmpegTranscodeType,
    utils::paths::CONFIG_DIR,
    video::{CaptureFramerate, TargetResolution},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CaptureConfig {
    pub record_cursor: bool,
    pub recording_border_indicator: bool,
    pub enable_ui_preview: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self { record_cursor: true, recording_border_indicator: true, enable_ui_preview: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct IdentityConfig {
    pub peer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NetworkConfig {
    pub max_depacket_latency: u16,
}

impl NetworkConfig {
    pub const DEFAULT_MAX_DEPACKET_LATENCY_MS: u16 = 50;
    pub const MAX_DEPACKET_LATENCY_MS: u16 = 1000;

    pub fn normalize(&mut self) {
        self.max_depacket_latency =
            self.max_depacket_latency.clamp(0, Self::MAX_DEPACKET_LATENCY_MS);
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self { max_depacket_latency: Self::DEFAULT_MAX_DEPACKET_LATENCY_MS }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoConfig {
    pub target_bitrate: u32,
    pub target_framerate: CaptureFramerate,
    pub target_resolution: TargetResolution,
    pub transcoding_type: FFmpegTranscodeType,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            target_bitrate: 8_000_000,
            target_framerate: CaptureFramerate::FPS60,
            target_resolution: TargetResolution::Source,
            transcoding_type: FFmpegTranscodeType::default(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Read(#[source] io::Error),
    #[error("Failed to parse config file: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Failed to save default config: {0}")]
    Save(#[source] io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Config {
    pub identity: IdentityConfig,
    pub video: VideoConfig,
    pub capture: CaptureConfig,
    pub network: NetworkConfig,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PersistedConfig {
    Legacy(LegacyConfig),
    Current(Config),
}

#[derive(Debug, Deserialize)]
pub struct LegacyConfig {
    pub peer_id: Option<String>,
    pub target_bitrate: u32,
    pub target_framerate: CaptureFramerate,
    pub target_resolution: TargetResolution,
    #[serde(default)]
    pub pixel_format: serde_json::Value,
    pub max_depacket_latency: u16,
    pub transcoding_type: FFmpegTranscodeType,
    pub record_cursor: bool,
    pub recording_border_indicator: bool,
    pub enable_ui_preview: bool,
}

impl From<LegacyConfig> for Config {
    fn from(legacy: LegacyConfig) -> Self {
        let _ = legacy.pixel_format;

        let mut config = Self {
            identity: IdentityConfig { peer_id: legacy.peer_id },
            video: VideoConfig {
                target_bitrate: legacy.target_bitrate,
                target_framerate: legacy.target_framerate,
                target_resolution: legacy.target_resolution,
                transcoding_type: legacy.transcoding_type,
            },
            capture: CaptureConfig {
                record_cursor: legacy.record_cursor,
                recording_border_indicator: legacy.recording_border_indicator,
                enable_ui_preview: legacy.enable_ui_preview,
            },
            network: NetworkConfig { max_depacket_latency: legacy.max_depacket_latency },
        };
        config.network.normalize();
        config
    }
}

impl Config {
    fn normalize(mut self) -> Self {
        self.network.normalize();
        self
    }

    fn get_config_path() -> PathBuf {
        CONFIG_DIR.join("config.json")
    }

    pub fn load() -> Result<Self, ConfigError> {
        tracing::info!("Loading config");
        let path = Self::get_config_path();
        if path.exists() {
            let content = fs::read(&path).map_err(ConfigError::Read)?;
            let persisted: PersistedConfig = serde_json::from_slice(&content)?;
            let config = match persisted {
                PersistedConfig::Current(config) => config,
                PersistedConfig::Legacy(config) => config.into(),
            };
            return Ok(config.normalize());
        }

        tracing::info!("No config file found, creating default config.");
        let default = Self::default().normalize();
        default.save().map_err(ConfigError::Save)?;
        Ok(default)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::get_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.clone().normalize())?;
        fs::write(path, content)?;

        Ok(())
    }
}

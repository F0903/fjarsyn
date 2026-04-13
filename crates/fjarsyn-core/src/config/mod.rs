use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    media::{
        ffmpeg::FFmpegTranscodeTypeExt,
        gpu_interop,
        pixel_format::PixelFormat,
        transcoding::FFmpegTranscodeType,
        video::{CaptureFramerate, TargetResolution},
    },
    utils::paths::CONFIG_DIR,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum PowerPref {
    #[default]
    Low,
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AppConfig {
    pub power_pref: PowerPref,
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
    pub app: AppConfig,
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
            app: AppConfig { power_pref: PowerPref::Low },
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
    pub fn normalized(mut self) -> Self {
        self.network.normalize();
        self
    }

    fn get_config_path() -> PathBuf {
        CONFIG_DIR.join("config.json")
    }

    // Tries to load the config from disk. If unable to load, a default config is created and saved.
    // If the default config is not able to be saved an error will be returned.
    pub fn load_or_overwrite() -> Result<Self, ConfigError> {
        tracing::info!("Loading config");
        let path = Self::get_config_path();
        if path.exists() {
            let content = fs::read(&path).map_err(ConfigError::Read)?;
            let persisted: PersistedConfig = serde_json::from_slice(&content)?;
            let config = match persisted {
                PersistedConfig::Current(config) => config,
                PersistedConfig::Legacy(config) => config.into(),
            };
            return Ok(config.normalized());
        }

        tracing::info!("No config file found, creating default config.");
        let default = Self::default().normalized();
        default.save().map_err(ConfigError::Save)?;
        Ok(default)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::get_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.clone().normalized())?;
        fs::write(path, content)?;

        Ok(())
    }
}

pub fn parse_target_bitrate_input(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map(|kbps| kbps * 1000)
        .map_err(|_| format!("Invalid bitrate value: '{}'", value))
}

pub fn clamp_max_depacket_latency(value: u16) -> u16 {
    value.clamp(0, NetworkConfig::MAX_DEPACKET_LATENCY_MS)
}

pub fn parse_max_depacket_latency_input(value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map(clamp_max_depacket_latency)
        .map_err(|_| format!("Invalid max depacket latency value: '{}'", value))
}

pub fn requires_capture_readback(config: &Config) -> bool {
    gpu_interop::requires_cpu_readback(
        config.capture.enable_ui_preview,
        PixelFormat::DEFAULT_CAPTURE,
        config.video.transcoding_type.get_encoder_info().hw_accel,
    )
}

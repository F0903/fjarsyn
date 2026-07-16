use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    identity::{LocalPeerIdentity, StoredIdentityKeypair},
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
    pub signing_key: Option<StoredIdentityKeypair>,
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
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub app: AppConfig,
    pub identity: IdentityConfig,
    pub video: VideoConfig,
    pub capture: CaptureConfig,
    pub network: NetworkConfig,
}

impl Config {
    fn ensure_signing_key(&mut self) -> bool {
        if self.identity.signing_key.is_some() {
            return false;
        }

        self.identity.signing_key = Some(LocalPeerIdentity::generate().to_stored());
        true
    }

    pub fn normalized(mut self) -> Self {
        self.network.normalize();
        self
    }

    fn get_config_path() -> PathBuf {
        CONFIG_DIR.join("config.json")
    }

    // Tries to load the config from disk. If unable to load, a default config is created and saved.
    // If the default config is not able to be saved an error will be returned.
    pub fn load_or_create() -> Result<Self, ConfigError> {
        tracing::info!("Loading config");
        let path = Self::get_config_path();
        if path.exists() {
            let content = fs::read(&path).map_err(ConfigError::Read)?;
            let mut config: Config = serde_json::from_slice(&content)?;
            config = config.normalized();
            if config.ensure_signing_key() {
                config.save().map_err(ConfigError::Save)?;
            }
            return Ok(config.normalized());
        }

        tracing::info!("No config file found, creating default config.");
        let mut default = Self::default().normalized();
        default.ensure_signing_key();
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
        .map_err(|_| format!("Invalid bitrate value: '{}'", value))?
        .checked_mul(1000)
        .ok_or_else(|| format!("Bitrate value is too large: '{}'", value))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_input_uses_checked_unit_conversion() {
        assert_eq!(parse_target_bitrate_input("8000"), Ok(8_000_000));
        assert!(parse_target_bitrate_input(&u32::MAX.to_string()).is_err());
    }

    #[test]
    fn network_latency_is_normalized_to_the_supported_limit() {
        let mut config = Config::default();
        config.network.max_depacket_latency = u16::MAX;

        assert_eq!(
            config.normalized().network.max_depacket_latency,
            NetworkConfig::MAX_DEPACKET_LATENCY_MS
        );
    }
}

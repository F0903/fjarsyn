//! Persisted application configuration, defaults, validation, and UI input parsing.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    identity::{LocalPeerIdentity, PeerId, StoredIdentityKeypair},
    media::{
        codec::TranscodeType,
        video::{Framerate, TargetResolution},
    },
    paths::CONFIG_DIR,
};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct IdentityConfig {
    pub peer_id: Option<PeerId>,
    pub signing_key: Option<StoredIdentityKeypair>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VideoConfig {
    pub target_bitrate: u32,
    pub target_framerate: Framerate,
    pub target_resolution: TargetResolution,
    pub transcoding_type: TranscodeType,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            target_bitrate: 8_000_000,
            target_framerate: Framerate::FPS60,
            target_resolution: TargetResolution::Source,
            transcoding_type: TranscodeType::default(),
        }
    }
}

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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to read config file: {0}")]
    Read(#[source] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Failed to save default config: {0}")]
    Save(#[source] std::io::Error),
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

    fn path() -> PathBuf {
        CONFIG_DIR.join("config.json")
    }

    /// Loads the persisted configuration, or creates and saves a normalized
    /// default when no configuration exists yet.
    pub fn load_or_create() -> Result<Self, Error> {
        tracing::info!("Loading config");
        let path = Self::path();
        if path.exists() {
            let content = fs::read(&path).map_err(Error::Read)?;
            let mut config = serde_json::from_slice::<Self>(&content)?.normalized();
            if config.ensure_signing_key() {
                config.save().map_err(Error::Save)?;
            }
            return Ok(config.normalized());
        }

        tracing::info!("No config file found, creating default config.");
        let mut config = Self::default().normalized();
        config.ensure_signing_key();
        config.save().map_err(Error::Save)?;
        Ok(config)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.clone().normalized())?;
        fs::write(path, content)
    }
}

pub fn parse_target_bitrate_input(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("Invalid bitrate value: '{value}'"))?
        .checked_mul(1000)
        .ok_or_else(|| format!("Bitrate value is too large: '{value}'"))
}

pub fn clamp_max_depacket_latency(value: u16) -> u16 {
    value.clamp(0, NetworkConfig::MAX_DEPACKET_LATENCY_MS)
}

pub fn parse_max_depacket_latency_input(value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map(clamp_max_depacket_latency)
        .map_err(|_| format!("Invalid max depacket latency value: '{value}'"))
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

    #[test]
    fn persisted_peer_ids_use_the_validated_identity_type() {
        let identity: IdentityConfig = serde_json::from_str(r#"{"peer_id":"peer-a"}"#).unwrap();
        assert_eq!(identity.peer_id, Some(PeerId::new("peer-a").unwrap()));

        assert!(serde_json::from_str::<IdentityConfig>(r#"{"peer_id":" peer-a"}"#).is_err());
    }
}

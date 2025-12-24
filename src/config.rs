use std::{fs, path::PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::capture_providers::shared::{CaptureFramerate, PixelFormat};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub onboarding_done: bool,
    pub server_url: String,
    pub bitrate: u32,
    pub framerate: CaptureFramerate,
    pub pixel_format: PixelFormat,
    pub max_depacket_latency: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            onboarding_done: false,
            bitrate: 8_000_000,
            framerate: CaptureFramerate::FPS30,
            server_url: "ws://127.0.0.1:30000/ws".to_string(),
            pixel_format: PixelFormat::RGBA8,
            max_depacket_latency: 2000,
        }
    }
}

impl Config {
    fn get_config_path() -> Option<PathBuf> {
        ProjectDirs::from("", "", "fjarsyn")
            .map(|proj_dirs| proj_dirs.config_dir().join("config.json"))
    }

    pub fn load() -> Self {
        tracing::info!("Loading config");
        if let Some(path) = Self::get_config_path() {
            if path.exists() {
                match fs::read(&path) {
                    Ok(content) => match serde_json::from_slice(&content) {
                        Ok(config) => return config,
                        Err(e) => tracing::error!("Failed to parse config file: {}", e),
                    },
                    Err(e) => tracing::error!("Failed to read config file: {}", e),
                }
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
        if let Some(path) = Self::get_config_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = serde_json::to_string_pretty(self)?;
            fs::write(path, content)?;
        }
        Ok(())
    }
}

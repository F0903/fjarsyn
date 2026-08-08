use crate::config::{CaptureConfig, VideoConfig};

/// Configuration owned by the screen-sharing capability.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    pub capture: CaptureConfig,
    pub video: VideoConfig,
}

impl From<&crate::config::Config> for Config {
    fn from(config: &crate::config::Config) -> Self {
        Self { capture: config.capture.clone(), video: config.video.clone() }
    }
}

impl From<crate::config::Config> for Config {
    fn from(config: crate::config::Config) -> Self {
        Self { capture: config.capture, video: config.video }
    }
}

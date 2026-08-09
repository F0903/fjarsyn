use crate::settings::{Capture, Settings, Video};

/// Live screen-sharing configuration accepted by [`super::ServiceHandle`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Config {
    pub capture: Capture,
    pub video: Video,
}

impl From<&Settings> for Config {
    fn from(settings: &Settings) -> Self {
        Self { capture: settings.capture.clone(), video: settings.video.clone() }
    }
}

impl From<Settings> for Config {
    fn from(settings: Settings) -> Self {
        Self { capture: settings.capture, video: settings.video }
    }
}

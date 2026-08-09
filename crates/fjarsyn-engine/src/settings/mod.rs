//! Secret-free settings consumed by the headless engine.

use serde::{Deserialize, Serialize};

mod capture;
mod error;
mod network;
mod video;

pub use capture::Capture;
pub use error::Error;
pub use network::Network;
pub use video::Video;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub video: Video,
    pub capture: Capture,
    pub network: Network,
}

impl Settings {
    pub fn normalized(mut self) -> Self {
        self.network.normalize();
        self
    }

    /// Validates settings at the engine's trust boundary.
    pub fn validate(&self) -> Result<(), Error> {
        self.video.validate()
    }

    /// Applies intentional normalization and rejects unsupported values.
    pub fn validated(self) -> Result<Self, Error> {
        let settings = self.normalized();
        settings.validate()?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_latency_is_normalized_to_the_supported_limit() {
        let mut settings = Settings::default();
        settings.network.max_depacket_latency_ms = u16::MAX;

        assert_eq!(
            settings.normalized().network.max_depacket_latency_ms,
            Network::MAX_DEPACKET_LATENCY_MS
        );
    }

    #[test]
    fn default_settings_are_valid() {
        assert_eq!(Settings::default().validated(), Ok(Settings::default()));
    }

    #[test]
    fn invalid_video_values_are_not_silently_normalized() {
        for bitrate in [0, Video::MIN_TARGET_BITRATE_BPS - 1, Video::MAX_TARGET_BITRATE_BPS + 1] {
            let mut settings = Settings::default();
            settings.video.target_bitrate_bps = bitrate;

            assert!(matches!(
                settings.validated(),
                Err(Error::TargetBitrateOutOfRange { value_bps, .. }) if value_bps == bitrate
            ));
        }

        let mut settings = Settings::default();
        settings.video.target_bitrate_bps += 1;
        assert!(matches!(
            settings.validated(),
            Err(Error::TargetBitrateNotWholeKilobitsPerSecond { value_bps: 8_000_001 })
        ));

        for (width, height) in [(0, 1080), (-1, 1080), (1920, -1), (4096, 2160), (1000, 1000)] {
            let mut settings = Settings::default();
            settings.video.target_resolution =
                crate::media::video::TargetResolution::Scale(crate::media::Dimensions {
                    width,
                    height,
                });

            assert!(matches!(
                settings.validated(),
                Err(Error::UnsupportedTargetResolution {
                    width: actual_width,
                    height: actual_height,
                }) if (actual_width, actual_height) == (width, height)
            ));
        }
    }

    #[test]
    fn unknown_fields_are_rejected_at_every_settings_boundary() {
        for json in [
            r#"{"unexpected":true}"#,
            r#"{"network":{"unexpected":true}}"#,
            r#"{"capture":{"unexpected":true}}"#,
            r#"{"video":{"unexpected":true}}"#,
        ] {
            assert!(serde_json::from_str::<Settings>(json).is_err(), "accepted {json}");
        }
    }
}

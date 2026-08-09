use serde::{Deserialize, Serialize};

use super::Error;
use crate::media::{
    codec::TranscodeType,
    video::{Framerate, TargetResolution},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Video {
    pub target_bitrate_bps: u32,
    pub target_framerate: Framerate,
    pub target_resolution: TargetResolution,
    pub transcoding_type: TranscodeType,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            target_bitrate_bps: 8_000_000,
            target_framerate: Framerate::FPS60,
            target_resolution: TargetResolution::Source,
            transcoding_type: TranscodeType::default(),
        }
    }
}

impl Video {
    /// Smallest supported encoder target, expressed in bits per second.
    pub const MIN_TARGET_BITRATE_BPS: u32 = 100_000;
    /// Largest supported encoder target, expressed in bits per second.
    pub const MAX_TARGET_BITRATE_BPS: u32 = 100_000_000;

    pub(crate) fn validate(&self) -> Result<(), Error> {
        if !(Self::MIN_TARGET_BITRATE_BPS..=Self::MAX_TARGET_BITRATE_BPS)
            .contains(&self.target_bitrate_bps)
        {
            return Err(Error::TargetBitrateOutOfRange {
                value_bps: self.target_bitrate_bps,
                min_bps: Self::MIN_TARGET_BITRATE_BPS,
                max_bps: Self::MAX_TARGET_BITRATE_BPS,
            });
        }
        if !self.target_bitrate_bps.is_multiple_of(1_000) {
            return Err(Error::TargetBitrateNotWholeKilobitsPerSecond {
                value_bps: self.target_bitrate_bps,
            });
        }

        if let TargetResolution::Scale(dimensions) = self.target_resolution
            && !TargetResolution::ALL.contains(&self.target_resolution)
        {
            return Err(Error::UnsupportedTargetResolution {
                width: dimensions.width,
                height: dimensions.height,
            });
        }

        Ok(())
    }
}

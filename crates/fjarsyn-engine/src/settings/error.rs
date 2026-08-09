#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(
        "target bitrate must be between {min_bps} and {max_bps} bits per second, but was {value_bps}"
    )]
    TargetBitrateOutOfRange { value_bps: u32, min_bps: u32, max_bps: u32 },
    #[error(
        "target bitrate must be a whole number of kilobits per second, but was {value_bps} bps"
    )]
    TargetBitrateNotWholeKilobitsPerSecond { value_bps: u32 },
    #[error("target resolution {width}x{height} is not one of the supported scaling presets")]
    UnsupportedTargetResolution { width: i32, height: i32 },
}

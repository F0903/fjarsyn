//! FFmpeg-specific metadata for the public codec selection.

use crate::media::codec::TranscodeType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::media::codec::backend) enum HardwareAcceleration {
    None,
    D3d11Va,
}

#[derive(Clone, Copy)]
pub(in crate::media::codec::backend) struct DecoderInfo {
    pub(in crate::media::codec::backend) name: &'static str,
    pub(in crate::media::codec::backend) hardware_acceleration: HardwareAcceleration,
}

#[derive(Clone, Copy)]
pub(in crate::media::codec::backend) struct EncoderInfo {
    pub(in crate::media::codec::backend) name: &'static str,
    pub(in crate::media::codec::backend) scaler_format: ffmpeg_next::util::format::Pixel,
    pub(in crate::media::codec::backend) hardware_acceleration: HardwareAcceleration,
}

impl TranscodeType {
    pub(in crate::media::codec::backend) fn encoder_info(self) -> EncoderInfo {
        match self {
            Self::H264Software => EncoderInfo {
                name: "libx264",
                scaler_format: ffmpeg_next::util::format::Pixel::YUV420P,
                hardware_acceleration: HardwareAcceleration::None,
            },
            Self::H264Nvenc => EncoderInfo {
                name: "h264_nvenc",
                scaler_format: ffmpeg_next::util::format::Pixel::BGRA,
                hardware_acceleration: HardwareAcceleration::D3d11Va,
            },
        }
    }

    pub(in crate::media::codec::backend) fn decoder_info(self) -> DecoderInfo {
        match self {
            Self::H264Software | Self::H264Nvenc => {
                DecoderInfo { name: "h264", hardware_acceleration: HardwareAcceleration::D3d11Va }
            }
        }
    }

    pub(in crate::media::codec::backend) fn set_encoder_options(
        self,
        opts: &mut ffmpeg_next::Dictionary,
    ) {
        opts.set("forced-idr", "1");
        match self {
            Self::H264Software => {
                opts.set("preset", "ultrafast");
                opts.set("tune", "zerolatency");
            }
            Self::H264Nvenc => {
                opts.set("preset", "p1");
                opts.set("tune", "ull");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_h264_encoder_uses_idr_frames_for_forced_keyframes() {
        for transcoding_type in [TranscodeType::H264Software, TranscodeType::H264Nvenc] {
            let mut options = ffmpeg_next::Dictionary::new();

            transcoding_type.set_encoder_options(&mut options);

            assert_eq!(options.get("forced-idr"), Some("1"));
        }
    }
}

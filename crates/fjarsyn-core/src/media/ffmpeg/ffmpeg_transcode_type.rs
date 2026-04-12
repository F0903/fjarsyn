pub use crate::transcoding::{FFmpegTranscodeType, HWAccelType};

#[derive(Clone, Copy)]
pub struct EncoderInfo {
    pub name: &'static str,
    pub input_format: ffmpeg_next::format::Pixel,
    pub scaler_format: ffmpeg_next::format::Pixel,
    pub hw_accel: HWAccelType,
}

#[derive(Clone, Copy)]
pub struct DecoderInfo {
    pub name: &'static str,
    pub hw_accel: HWAccelType,
}

pub trait FFmpegTranscodeTypeExt {
    fn get_encoder_info(&self) -> EncoderInfo;
    fn get_decoder_info(&self) -> DecoderInfo;
    fn set_encoder_options(&self, opts: &mut ffmpeg_next::Dictionary);
}

impl FFmpegTranscodeTypeExt for FFmpegTranscodeType {
    fn get_encoder_info(&self) -> EncoderInfo {
        match self {
            FFmpegTranscodeType::H264Software => EncoderInfo {
                name: "libx264",
                input_format: ffmpeg_next::format::Pixel::YUV420P,
                scaler_format: ffmpeg_next::format::Pixel::YUV420P,
                hw_accel: HWAccelType::None,
            },
            FFmpegTranscodeType::H264Nvenc => EncoderInfo {
                name: "h264_nvenc",
                input_format: ffmpeg_next::format::Pixel::BGRA,
                scaler_format: ffmpeg_next::format::Pixel::BGRA,
                hw_accel: HWAccelType::D3D11VA,
            },
        }
    }

    fn get_decoder_info(&self) -> DecoderInfo {
        match self {
            FFmpegTranscodeType::H264Software | FFmpegTranscodeType::H264Nvenc => {
                DecoderInfo { name: "h264", hw_accel: HWAccelType::D3D11VA }
            }
        }
    }

    fn set_encoder_options(&self, opts: &mut ffmpeg_next::Dictionary) {
        match self {
            FFmpegTranscodeType::H264Software => {
                opts.set("preset", "ultrafast");
                opts.set("tune", "zerolatency");
            }
            FFmpegTranscodeType::H264Nvenc => {
                opts.set("preset", "p1");
                opts.set("tune", "ull");
            }
        }
    }
}

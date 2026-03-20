use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HWAccelType {
    None,
    D3D11VA,
}

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

macro_rules! define_ffmpeg_transcode_types {
    (
        $(
            $variant:ident $( => $def:tt )? {
                encoder: {
                    name: $encoder_name:expr,
                    set_options: $set_encoder_options:expr,
                    input_format: $input_format:expr,
                    scaler_format: $scaler_format:expr,
                    hw_accel: $hw_accel:expr,
                },
                decoder: {
                    name: $decoder_name:expr,
                    hw_accel: $decoder_hw_accel:expr,
                }
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
        pub enum FFmpegTranscodeType {
            $(
                $variant,
            )*
        }

        impl Default for FFmpegTranscodeType {
            fn default() -> Self {
                $(
                    $(
                        let _ = stringify!($def);
                        return FFmpegTranscodeType::$variant;
                    )?
                )*
                // If no explicit default is marked, fall back to the first variant.
                #[allow(unreachable_code)]
                if let Some(first) = Self::ALL.first() {
                    *first
                } else {
                    panic!("No variants defined for FFmpegTranscodeType");
                }
            }
        }

        impl FFmpegTranscodeType {
            pub const ALL: &'static [Self] = &[
                $(
                    Self::$variant,
                )*
            ];

            pub const fn get_encoder_info(&self) -> EncoderInfo {
                match self {
                    $(
                        FFmpegTranscodeType::$variant => EncoderInfo {
                            name: $encoder_name,
                            input_format: $input_format,
                            scaler_format: $scaler_format,
                            hw_accel: $hw_accel,
                        },
                    )*
                }
            }

            pub const fn get_decoder_info(&self) -> DecoderInfo {
                match self {
                    $(
                        FFmpegTranscodeType::$variant => DecoderInfo {
                            name: $decoder_name,
                            hw_accel: $decoder_hw_accel,
                        },
                    )*
                }
            }

            pub fn set_encoder_options(&self, opts: &mut ffmpeg_next::Dictionary) {
                match self {
                    $(
                        FFmpegTranscodeType::$variant => {
                            $set_encoder_options(opts);
                        }
                    )*
                }
            }
        }

        impl std::fmt::Display for FFmpegTranscodeType {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let label = match self {
                    FFmpegTranscodeType::H264Software => "H.264 (Software Encode)",
                    FFmpegTranscodeType::H264Nvenc => "H.264 (NVIDIA Encode)",
                };

                f.write_str(label)
            }
        }
    };
}

define_ffmpeg_transcode_types! {
    H264Software => default {
        encoder: {
            name: "libx264",
            set_options: |opts: &mut ffmpeg_next::Dictionary| {
                opts.set("preset", "ultrafast");
                opts.set("tune", "zerolatency");
            },
            input_format: ffmpeg_next::format::Pixel::YUV420P,
            scaler_format: ffmpeg_next::format::Pixel::YUV420P,
            hw_accel: HWAccelType::None,
        },
        decoder: {
            name: "h264",
            hw_accel: HWAccelType::D3D11VA,
        }
    },
    H264Nvenc {
        encoder: {
            name: "h264_nvenc",
            set_options: |opts: &mut ffmpeg_next::Dictionary| {
                opts.set("preset", "p1");
                opts.set("tune", "ull");
            },
            input_format: ffmpeg_next::format::Pixel::BGRA,
            scaler_format: ffmpeg_next::format::Pixel::BGRA,
            hw_accel: HWAccelType::D3D11VA,
        },
        decoder: {
            name: "h264",
            hw_accel: HWAccelType::D3D11VA,
        }
    },
}

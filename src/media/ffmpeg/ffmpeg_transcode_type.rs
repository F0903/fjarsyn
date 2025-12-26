use serde::{Deserialize, Serialize};

macro_rules! define_ffmpeg_transcode_types {
    (
        $(
            $variant:ident $( => $def:tt )? {
                encoder_name: $encoder_name:expr,
                set_encoder_options: $set_encoder_options:expr,
                decoder_name: $decoder_name:expr,
                input_format: $input_format:expr,
                hw_accel_name: $hw_accel_name:expr,
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

            pub fn to_encoder_name(&self) -> &'static str {
                match self {
                    $(
                        FFmpegTranscodeType::$variant => $encoder_name,
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

            pub fn to_decoder_name(&self) -> &'static str {
                match self {
                    $(
                        FFmpegTranscodeType::$variant => $decoder_name,
                    )*
                }
            }

            pub fn hw_accel_name(&self) -> Option<&'static str> {
                match self {
                    $(
                        FFmpegTranscodeType::$variant => $hw_accel_name,
                    )*
                }
            }

            pub fn get_input_format(&self) -> ffmpeg_next::format::Pixel {
                match self {
                    $(
                        FFmpegTranscodeType::$variant => $input_format,
                    )*
                }
            }
        }

        impl std::fmt::Display for FFmpegTranscodeType {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:?}", self)
            }
        }
    };
}

define_ffmpeg_transcode_types! {
    H264Software => default {
        encoder_name: "libx264",
        set_encoder_options: |opts: &mut ffmpeg_next::Dictionary| {
            opts.set("preset", "ultrafast");
            opts.set("tune", "zerolatency");
        },
        decoder_name: "h264",
        input_format: ffmpeg_next::format::Pixel::YUV420P,
        hw_accel_name: None,
    },
    H264Vulkan {
        encoder_name: "h264_vulkan",
        set_encoder_options: |opts: &mut ffmpeg_next::Dictionary| {
            opts.set("tune", "ull");
            opts.set("usage", "conference");
            opts.set("content", "desktop");
        },
        decoder_name: "h264",
        input_format: ffmpeg_next::format::Pixel::VULKAN,
        hw_accel_name: Some("vulkan"),
    },
    H265Vulkan {
        encoder_name: "hevc_vulkan",
        set_encoder_options: |opts: &mut ffmpeg_next::Dictionary| {
            opts.set("tune", "ull");
            opts.set("usage", "conference");
            opts.set("content", "desktop");
        },
        decoder_name: "hevc",
        input_format: ffmpeg_next::format::Pixel::VULKAN,
        hw_accel_name: Some("vulkan"),
    },
}

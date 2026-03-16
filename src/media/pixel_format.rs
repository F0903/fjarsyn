use serde::{Deserialize, Serialize};
use windows::Graphics::DirectX::DirectXPixelFormat;

macro_rules! define_pixel_formats {
    (
        $(
            $variant:ident {
                bytes: $bytes:expr,
                directx: $directx:expr,
                ffmpeg: $ffmpeg:expr $(,)?
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
        pub enum PixelFormat {
            $(
                $variant,
            )*
        }

        impl PixelFormat {
            pub const fn bytes_per_pixel(&self) -> u32 {
                match self {
                    $(
                        PixelFormat::$variant => $bytes,
                    )*
                }
            }

            pub const fn to_directx_pixel_format(&self) -> DirectXPixelFormat {
                match self {
                    $(
                        PixelFormat::$variant => $directx,
                    )*
                }
            }

            pub const fn to_ffmpeg_pixel_format(&self) -> ffmpeg_next::format::Pixel {
                match self {
                    $(
                        PixelFormat::$variant => $ffmpeg,
                    )*
                }
            }

            pub const fn is_iced_compatible(&self) -> bool {
                match self {
                    PixelFormat::RGBA8 | PixelFormat::BGRA8 => true,
                    _ => false,
                }
            }
        }
    };
}

define_pixel_formats! {
    RGBA10 {
        bytes: 4,
        directx: DirectXPixelFormat::R10G10B10A2UIntNormalized,
        ffmpeg: ffmpeg_next::format::Pixel::X2RGB10LE,
    },
    RGBA16 {
        bytes: 8,
        directx: DirectXPixelFormat::R16G16B16A16Float,
        ffmpeg: ffmpeg_next::format::Pixel::RGBAF16LE,
    },
    RGBA8 {
        bytes: 4,
        directx: DirectXPixelFormat::R8G8B8A8UIntNormalized,
        ffmpeg: ffmpeg_next::format::Pixel::RGBA,
    },
    BGRA8 {
        bytes: 4,
        directx: DirectXPixelFormat::B8G8R8A8UIntNormalized,
        ffmpeg: ffmpeg_next::format::Pixel::BGRA,
    },
    NV12 {
        bytes: 2, // Average bytes per pixel
        directx: DirectXPixelFormat::NV12,
        ffmpeg: ffmpeg_next::format::Pixel::NV12,
    }
}

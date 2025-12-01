use windows::Graphics::DirectX::DirectXPixelFormat;

#[derive(Debug, Clone)]
pub enum PixelFormat {
    RGBA16,
    RGBA8,
    BGRA8,
}

pub trait BytesPerPixel {
    fn bytes_per_pixel(&self) -> u32;
}

impl BytesPerPixel for PixelFormat {
    fn bytes_per_pixel(&self) -> u32 {
        match self {
            PixelFormat::RGBA16 => 8,
            PixelFormat::RGBA8 => 4,
            PixelFormat::BGRA8 => 4,
        }
    }
}

pub trait ToDirectXPixelFormat {
    fn to_directx_pixel_format(&self) -> DirectXPixelFormat;
}

impl ToDirectXPixelFormat for PixelFormat {
    fn to_directx_pixel_format(&self) -> DirectXPixelFormat {
        match self {
            PixelFormat::RGBA16 => DirectXPixelFormat::R16G16B16A16Float,
            PixelFormat::RGBA8 => DirectXPixelFormat::R8G8B8A8UIntNormalized,
            PixelFormat::BGRA8 => DirectXPixelFormat::B8G8R8A8UIntNormalized,
        }
    }
}

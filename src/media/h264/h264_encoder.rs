use openh264::{
    encoder::{Encoder, EncoderConfig},
    formats::YUVBuffer,
};

use crate::utils::bitmap_utils::rgba8_to_yuv420;

type Result<T> = std::result::Result<T, H264EncoderError>;

#[derive(Debug, thiserror::Error)]
pub enum H264EncoderError {
    #[error("Failed to create encoder")]
    CreateEncoderError(openh264::Error),
    #[error("Failed to encode frame")]
    EncodeError(openh264::Error),
}

/// A simple H.264 encoder.
pub struct H264Encoder {
    encoder: Encoder,
}

impl std::fmt::Debug for H264Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("H264Encoder"))
    }
}

impl H264Encoder {
    /// Creates a new H.264 encoder with the given dimensions.
    pub fn new() -> Result<Self> {
        let encoder = Encoder::with_api_config(
            openh264::OpenH264API::from_source(),
            EncoderConfig::default(),
        )
        .map_err(H264EncoderError::CreateEncoderError)?;
        Ok(Self { encoder })
    }

    /// Encodes a raw RGB8 bitmap into a list of H.264 NAL units.
    pub fn encode(&mut self, bitmap: &[u8], width: i32, height: i32) -> Result<Vec<Vec<u8>>> {
        let mut yuv_vec = vec![0u8; (width * height * 3 / 2) as usize];
        rgba8_to_yuv420(bitmap, width as usize, height as usize, &mut yuv_vec);
        let yuv = YUVBuffer::from_vec(yuv_vec, width as usize, height as usize);

        let bitstream = self.encoder.encode(&yuv).map_err(H264EncoderError::EncodeError)?;
        let bitstream_bytes = bitstream.to_vec();
        Ok(openh264::nal_units(&bitstream_bytes).map(|nal| nal.to_vec()).collect())
    }
}

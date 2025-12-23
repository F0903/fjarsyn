use openh264::{
    encoder::{BitRate, Encoder, EncoderConfig, FrameRate},
    formats::YUVBuffer,
};

use crate::utils::bitmap_utils::rgba8_to_yuv420;

type Result<T> = std::result::Result<T, H264EncoderError>;

#[derive(Debug, thiserror::Error)]
pub enum H264EncoderError {
    #[error("Failed to create encoder: {0}")]
    CreateEncoderError(openh264::Error),
    #[error("Failed to encode frame: {0}")]
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
    pub fn new(bitrate: u32, target_framerate_hz: f32) -> Result<Self> {
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(target_framerate_hz))
            .skip_frames(true);
        let encoder = Encoder::with_api_config(openh264::OpenH264API::from_source(), config)
            .map_err(H264EncoderError::CreateEncoderError)?;
        Ok(Self { encoder })
    }

    /// Encodes a raw RGB8 bitmap into a list of H.264 NAL units.
    pub fn encode(&mut self, bitmap: &[u8], width: i32, height: i32) -> Result<Vec<Vec<u8>>> {
        // Ensure width and height are even
        let aligned_width = (width as usize) & !1;
        let aligned_height = (height as usize) & !1;

        let yuv_vec_len = (aligned_width * aligned_height * 3 / 2) as usize;
        let mut yuv_vec = Vec::with_capacity(yuv_vec_len);
        unsafe { yuv_vec.set_len(yuv_vec_len) };

        rgba8_to_yuv420(bitmap, aligned_width, aligned_height, width as usize, &mut yuv_vec);
        let yuv = YUVBuffer::from_vec(yuv_vec, aligned_width, aligned_height);

        let bitstream = self.encoder.encode(&yuv).map_err(H264EncoderError::EncodeError)?;
        let bitstream_bytes = bitstream.to_vec();
        Ok(openh264::nal_units(&bitstream_bytes).map(|nal| nal.to_vec()).collect())
    }
}

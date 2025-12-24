use std::sync::Arc;

use openh264::decoder::{DecodeOptions, Decoder, DecoderConfig};

use crate::{
    capture_providers::shared::{Frame, PixelFormat, Vector2},
    utils::buffer_arena::BufferArena,
};

type Result<T> = std::result::Result<T, H264DecoderError>;

#[derive(Debug, thiserror::Error)]
pub enum H264DecoderError {
    #[error("Failed to create decoder: {0}")]
    CreateDecoderError(openh264::Error),
    #[error("Failed to decode frame: {0}")]
    DecodeError(openh264::Error),
}

pub struct H264Decoder {
    decoder: Decoder,
    decoding_pool: BufferArena,
}

impl H264Decoder {
    const POOL_SIZE: usize = 4;

    pub fn new() -> Result<Self> {
        let decoder = Decoder::with_api_config(
            openh264::OpenH264API::from_source(),
            DecoderConfig::default(),
        )
        .map_err(H264DecoderError::CreateDecoderError)?;
        Ok(Self { decoder, decoding_pool: BufferArena::init(Self::POOL_SIZE) })
    }

    pub fn decode(&mut self, packet: &[u8]) -> Result<Option<Arc<Frame>>> {
        let Some(image) = self
            .decoder
            .decode_with_options(packet, DecodeOptions::default())
            .map_err(H264DecoderError::DecodeError)?
        else {
            return Ok(None);
        };

        let image_dims_uv = image.dimensions_uv();
        let est_image_width = image_dims_uv.0 * 2;
        let est_image_height = image_dims_uv.1 * 2;
        let size = est_image_width * est_image_height * 4;
        tracing::debug!("Decoding frame with size: {} x {}", est_image_width, est_image_height);

        let mut framebuf = self.decoding_pool.get(size);
        image.write_rgba8(&mut framebuf);

        let frame = Arc::new(Frame::new_raw(
            framebuf,
            PixelFormat::RGBA8, // We currently assume remote frames are always RGBA8
            Vector2::<i32>::new(est_image_width as i32, est_image_height as i32),
            None,
            None,
        ));

        Ok(Some(frame))
    }
}

impl std::fmt::Debug for H264Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H264Decoder")
            .field("decoder", &"<Decoder>".to_owned())
            .field("decoding_pool", &self.decoding_pool)
            .finish()
    }
}

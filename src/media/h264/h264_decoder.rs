use openh264::decoder::{DecodeOptions, Decoder, DecoderConfig};

type Result<T> = std::result::Result<T, H264DecoderError>;

#[derive(Debug, thiserror::Error)]
pub enum H264DecoderError {
    #[error("Failed to create decoder")]
    CreateDecoderError(openh264::Error),
    #[error("Failed to decode frame")]
    DecodeError(openh264::Error),
}

pub struct H264Decoder {
    decoder: Decoder,
}

impl H264Decoder {
    pub fn new() -> Result<Self> {
        let decoder = Decoder::with_api_config(
            openh264::OpenH264API::from_source(),
            DecoderConfig::default(),
        )
        .map_err(H264DecoderError::CreateDecoderError)?;
        Ok(Self { decoder })
    }

    pub fn decode(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>> {
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
        let mut framebuf = Vec::with_capacity(est_image_width * est_image_height);
        image.write_rgba8(&mut framebuf);

        Ok(Some(framebuf))
    }
}

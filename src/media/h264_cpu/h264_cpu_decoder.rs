use std::sync::Arc;

use ffmpeg::{
    Packet, codec, decoder, format, frame,
    software::scaling::{self, Context as Scaler},
};
use ffmpeg_next as ffmpeg;

use crate::{
    capture_providers::shared::{Frame, PixelFormat, Vector2},
    utils::buffer_arena::BufferArena,
};

type Result<T> = std::result::Result<T, H264DecoderError>;

#[derive(Debug, thiserror::Error)]
pub enum H264DecoderError {
    #[error("Failed to create decoder: {0}")]
    CreateDecoderError(ffmpeg::Error),
    #[error("Failed to decode frame: {0}")]
    DecodeError(ffmpeg::Error),
    #[error("Failed to convert frame: {0}")]
    ConversionError(ffmpeg::Error),
    #[error("Failed to initialize scaler: {0}")]
    ScalerError(ffmpeg::Error),
}

pub struct H264Decoder {
    decoder: decoder::Video,
    scaler: Option<Scaler>,
    decoding_pool: BufferArena,
    cached_dims: (u32, u32),
}

impl H264Decoder {
    const POOL_SIZE: usize = 128000;

    pub fn new() -> Result<Self> {
        ffmpeg::init().map_err(H264DecoderError::CreateDecoderError)?;

        let codec = codec::decoder::find(codec::Id::H264)
            .ok_or(H264DecoderError::CreateDecoderError(ffmpeg::Error::DecoderNotFound))?;

        let context = codec::context::Context::new_with_codec(codec)
            .decoder()
            .video()
            .map_err(H264DecoderError::CreateDecoderError)?;

        // We don't set width/height here as they are extracted from the stream.

        Ok(Self {
            decoder: context,
            scaler: None,
            decoding_pool: BufferArena::init(Self::POOL_SIZE),
            cached_dims: (0, 0),
        })
    }

    pub fn decode(&mut self, packet_data: &[u8]) -> Result<Option<Arc<Frame>>> {
        let mut packet = Packet::new(packet_data.len());
        packet.data_mut().unwrap().copy_from_slice(packet_data);

        self.decoder.send_packet(&packet).map_err(H264DecoderError::DecodeError)?;

        let mut decoded_frame = frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded_frame) {
            Ok(_) => {
                let width = decoded_frame.width();
                let height = decoded_frame.height();
                let format = decoded_frame.format();

                // Initialize or update scaler if dimensions changed
                if self.scaler.is_none() || self.cached_dims != (width, height) {
                    tracing::debug!("Initializing scaler for {}x{}", width, height);
                    let scaler = scaling::Context::get(
                        format,
                        width,
                        height,
                        format::Pixel::RGBA,
                        width,
                        height,
                        scaling::Flags::BILINEAR,
                    )
                    .map_err(H264DecoderError::ScalerError)?;

                    self.scaler = Some(scaler);
                    self.cached_dims = (width, height);
                }

                let scaler = self.scaler.as_mut().unwrap();
                let mut rgb_frame = frame::Video::new(format::Pixel::RGBA, width, height);

                scaler
                    .run(&decoded_frame, &mut rgb_frame)
                    .map_err(H264DecoderError::ConversionError)?;

                // Copy to buffer arena
                let size = (width * height * 4) as usize;
                let mut framebuf = self.decoding_pool.get(size);

                // RGBA is packed, so we can copy from the first plane
                let data = rgb_frame.data(0);
                let linesize = rgb_frame.stride(0);

                // Copy row by row to handle stride
                let dest_stride = (width * 4) as usize;
                for i in 0..height as usize {
                    let src_start = i * linesize;
                    let src_end = src_start + dest_stride;
                    let dest_start = i * dest_stride;
                    let dest_end = dest_start + dest_stride;

                    framebuf[dest_start..dest_end].copy_from_slice(&data[src_start..src_end]);
                }

                let frame = Arc::new(Frame::new_raw(
                    framebuf,
                    PixelFormat::RGBA8,
                    Vector2::<i32>::new(width as i32, height as i32),
                    None,
                    None,
                ));

                Ok(Some(frame))
            }
            Err(ffmpeg::Error::Other { errno: ffmpeg::error::EAGAIN }) => {
                // Need more data
                Ok(None)
            }
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(H264DecoderError::DecodeError(e)),
        }
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

unsafe impl Send for H264Decoder {}

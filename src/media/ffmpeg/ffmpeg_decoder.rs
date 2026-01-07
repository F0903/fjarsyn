use std::sync::Arc;

use ffmpeg::{
    Packet, codec, decoder, frame,
    software::scaling::{self, Context as Scaler},
};
use ffmpeg_next as ffmpeg;

use crate::{
    media::ffmpeg::FFmpegTranscodeType,
    utils::{buffer_arena::BufferArena, frame::Frame, pixel_format::PixelFormat, vector2::Vector2},
};

type Result<T> = std::result::Result<T, FFmpegDecoderError>;

#[derive(Debug, thiserror::Error)]
pub enum FFmpegDecoderError {
    #[error("Failed to create decoder: {0}")]
    CreateDecoderError(ffmpeg::Error),
    #[error("Failed to decode frame: {0}")]
    DecodeError(ffmpeg::Error),
    #[error("Failed to convert frame: {0}")]
    ConversionError(ffmpeg::Error),
    #[error("Failed to initialize scaler: {0}")]
    ScalerError(ffmpeg::Error),
}

pub struct FFmpegDecoder {
    dst_format: PixelFormat,
    decoder: decoder::Video,
    scaler: Option<Scaler>,
    decoding_pool: BufferArena,
    cached_dims: (u32, u32),
}

impl FFmpegDecoder {
    const POOL_SIZE: usize = 128000;
    const SCALING_MODE: scaling::Flags = scaling::Flags::BILINEAR;

    pub fn new(transcoding_type: FFmpegTranscodeType, dst_format: PixelFormat) -> Result<Self> {
        ffmpeg::init().map_err(FFmpegDecoderError::CreateDecoderError)?;

        let decoder_info = transcoding_type.get_decoder_info();

        let codec = codec::decoder::find_by_name(decoder_info.name)
            .ok_or(FFmpegDecoderError::CreateDecoderError(ffmpeg::Error::DecoderNotFound))?;

        let mut context = codec::context::Context::new_with_codec(codec);
        context.set_flags(codec::Flags::LOW_DELAY);

        let decoder = context
            .decoder()
            .open_as(codec)
            .and_then(|d| d.video())
            .map_err(FFmpegDecoderError::CreateDecoderError)?;

        Ok(Self {
            dst_format,
            decoder,
            scaler: None,
            decoding_pool: BufferArena::init(Self::POOL_SIZE),
            cached_dims: (0, 0),
        })
    }

    pub fn decode(&mut self, packet_data: &[u8]) -> Result<Option<Arc<Frame>>> {
        let packet = Packet::borrow(packet_data);
        self.decoder.send_packet(&packet).map_err(FFmpegDecoderError::DecodeError)?;

        let mut decoded_frame = frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded_frame) {
            Ok(_) => {
                let width = decoded_frame.width();
                let height = decoded_frame.height();
                let format = decoded_frame.format();

                let ffmpeg_pixel_format = self.dst_format.to_ffmpeg_pixel_format();

                // Initialize or update scaler if dimensions changed
                if self.scaler.is_none() || self.cached_dims != (width, height) {
                    tracing::debug!("Initializing scaler for {}x{}", width, height);
                    let scaler = scaling::Context::get(
                        format,
                        width,
                        height,
                        ffmpeg_pixel_format,
                        width,
                        height,
                        Self::SCALING_MODE,
                    )
                    .map_err(FFmpegDecoderError::ScalerError)?;

                    self.scaler = Some(scaler);
                    self.cached_dims = (width, height);
                }

                let scaler = self.scaler.as_mut().unwrap();
                let mut rgb_frame = frame::Video::new(ffmpeg_pixel_format, width, height);

                scaler
                    .run(&decoded_frame, &mut rgb_frame)
                    .map_err(FFmpegDecoderError::ConversionError)?;

                let dst_bytes_per_pixel = self.dst_format.bytes_per_pixel();
                let dst_size = (width * height * dst_bytes_per_pixel) as usize;
                let mut framebuf = self.decoding_pool.get(dst_size);

                // Copy from the first plane
                let data = rgb_frame.data(0);
                let linesize = rgb_frame.stride(0);

                // Copy row by row to handle stride
                let dst_stride = (width * dst_bytes_per_pixel) as usize;
                for i in 0..height as usize {
                    let src_start = i * linesize;
                    let src_end = src_start + dst_stride;
                    let dst_start = i * dst_stride;
                    let dst_end = dst_start + dst_stride;

                    framebuf[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
                }

                let frame = Arc::new(Frame::new_raw(
                    framebuf,
                    self.dst_format,
                    Vector2::new(width as i32, height as i32),
                    None,
                ));

                Ok(Some(frame))
            }
            Err(ffmpeg::Error::Other { errno: ffmpeg::error::EAGAIN }) => {
                // Need more data
                Ok(None)
            }
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(FFmpegDecoderError::DecodeError(e)),
        }
    }
}

impl std::fmt::Debug for FFmpegDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FFmpegDecoder")
            .field("decoder", &"<Decoder>".to_owned())
            .field("decoding_pool", &self.decoding_pool)
            .finish()
    }
}

unsafe impl Send for FFmpegDecoder {}

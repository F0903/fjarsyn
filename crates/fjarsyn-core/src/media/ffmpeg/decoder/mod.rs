use std::sync::Arc;

use ffmpeg::{
    Packet, codec, decoder, format, frame,
    software::scaling::{self, Context as Scaler},
};
use ffmpeg_next as ffmpeg;

mod hw;

use crate::{
    media::{
        ffmpeg::ffmpeg_transcode_type::{DecoderInfo, FFmpegTranscodeType, FFmpegTranscodeTypeExt},
        frame::Frame,
        pixel_format::PixelFormat,
    },
    utils::{buffer_pool::BufferPool, vector2::Vector2},
};

type Result<T> = std::result::Result<T, FFmpegDecoderError>;

#[derive(Debug, thiserror::Error)]
pub enum FFmpegDecoderError {
    #[error("Failed to create decoder: {0}")]
    Create(ffmpeg::Error),
    #[error("Failed to decode frame: {0}")]
    Decode(ffmpeg::Error),
    #[error("Failed to convert frame: {0}")]
    Conversion(ffmpeg::Error),
    #[error("Failed to initialize scaler: {0}")]
    Scaler(ffmpeg::Error),
    #[error("Failed to process hardware frame: {0}")]
    HardwareInterop(String),
}

pub struct FFmpegDecoder {
    dst_format: PixelFormat,
    decoder: decoder::Video,
    scaler: Option<Scaler>,
    decoding_pool: BufferPool,
    cached_dims: (u32, u32),
    cached_src_format: Option<format::Pixel>,
    hw_backend: Option<hw::HwDecoderBackend>,
}

impl FFmpegDecoder {
    const BUFFER_SIZE: usize = 128000;
    const BUFFER_MAX_COUNT: usize = 4;
    const SCALING_MODE: scaling::Flags = scaling::Flags::BILINEAR;

    pub fn new(transcoding_type: FFmpegTranscodeType, dst_format: PixelFormat) -> Result<Self> {
        ffmpeg::init().map_err(FFmpegDecoderError::Create)?;

        let decoder_info = transcoding_type.get_decoder_info();
        let codec = codec::decoder::find_by_name(decoder_info.name)
            .ok_or(FFmpegDecoderError::Create(ffmpeg::Error::DecoderNotFound))?;
        let (decoder, hw_backend) = hw::HwDecoderBackend::open_decoder(codec, decoder_info)?;

        Ok(Self {
            dst_format,
            decoder,
            scaler: None,
            decoding_pool: BufferPool::init(Self::BUFFER_SIZE, Self::BUFFER_MAX_COUNT),
            cached_dims: (0, 0),
            cached_src_format: None,
            hw_backend,
        })
    }

    pub fn decode(&mut self, packet_data: &[u8]) -> Result<Option<Arc<Frame>>> {
        let packet = Packet::borrow(packet_data);
        self.decoder.send_packet(&packet).map_err(FFmpegDecoderError::Decode)?;

        let mut decoded_frame = frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded_frame) {
            Ok(_) => {
                if let Some(frame) = self.try_decode_hw_frame(&decoded_frame) {
                    return frame.map(Some);
                }

                self.decode_software_frame(decoded_frame).map(Some)
            }
            Err(ffmpeg::Error::Other { errno: ffmpeg::error::EAGAIN }) => Ok(None),
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(FFmpegDecoderError::Decode(e)),
        }
    }

    fn try_decode_hw_frame(&self, decoded_frame: &frame::Video) -> Option<Result<Arc<Frame>>> {
        let hw_backend = self.hw_backend.as_ref()?;

        match hw_backend.try_decode_frame(decoded_frame, self.dst_format) {
            Ok(Some(frame)) => Some(Ok(Arc::new(frame))),
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(
                    "Falling back to software decode output after GPU path failed: {}",
                    err
                );
                None
            }
        }
    }

    fn decode_software_frame(&mut self, decoded_frame: frame::Video) -> Result<Arc<Frame>> {
        let source_frame = self.materialize_source_frame(decoded_frame)?;
        let width = source_frame.width();
        let height = source_frame.height();
        let src_format = source_frame.format();

        let mut rgb_frame =
            frame::Video::new(self.dst_format.to_ffmpeg_pixel_format(), width, height);
        self.ensure_scaler(width, height, src_format)?
            .run(&source_frame, &mut rgb_frame)
            .map_err(FFmpegDecoderError::Conversion)?;

        Ok(Arc::new(self.copy_software_frame(&rgb_frame, width, height)))
    }

    fn materialize_source_frame(&self, decoded_frame: frame::Video) -> Result<frame::Video> {
        if let Some(hw_backend) = &self.hw_backend {
            hw_backend.materialize_frame(decoded_frame)
        } else {
            Ok(decoded_frame)
        }
    }

    fn ensure_scaler(
        &mut self,
        width: u32,
        height: u32,
        src_format: format::Pixel,
    ) -> Result<&mut Scaler> {
        let ffmpeg_pixel_format = self.dst_format.to_ffmpeg_pixel_format();

        if self.scaler.is_none()
            || self.cached_dims != (width, height)
            || self.cached_src_format != Some(src_format)
        {
            tracing::debug!(
                "Initializing scaler for {}x{} from {:?} to {:?}",
                width,
                height,
                src_format,
                ffmpeg_pixel_format
            );
            let scaler = scaling::Context::get(
                src_format,
                width,
                height,
                ffmpeg_pixel_format,
                width,
                height,
                Self::SCALING_MODE,
            )
            .map_err(FFmpegDecoderError::Scaler)?;

            self.scaler = Some(scaler);
            self.cached_dims = (width, height);
            self.cached_src_format = Some(src_format);
        }

        Ok(self.scaler.as_mut().expect("scaler must be initialized"))
    }

    fn copy_software_frame(&mut self, rgb_frame: &frame::Video, width: u32, height: u32) -> Frame {
        let dst_bytes_per_pixel = self.dst_format.bytes_per_pixel();
        let dst_size = (width * height * dst_bytes_per_pixel) as usize;
        let mut framebuf = self.decoding_pool.get_unzeroed(dst_size);

        let data = rgb_frame.data(0);
        let linesize = rgb_frame.stride(0);
        let dst_stride = (width * dst_bytes_per_pixel) as usize;
        for i in 0..height as usize {
            let src_start = i * linesize;
            let src_end = src_start + dst_stride;
            let dst_start = i * dst_stride;
            let dst_end = dst_start + dst_stride;

            framebuf[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
        }

        Frame::new_software(
            framebuf,
            self.dst_format,
            Vector2::new(width as i32, height as i32),
            None,
        )
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

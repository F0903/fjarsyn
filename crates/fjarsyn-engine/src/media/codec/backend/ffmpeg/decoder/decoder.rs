use std::sync::Arc;

use ffmpeg::{
    Packet, codec, decoder, frame,
    software::scaling::{self, Context as Scaler},
    util::format,
};
use ffmpeg_next as ffmpeg;

use super::hw;
use crate::media::{
    Dimensions, PixelFormat, buffer_pool::Pool, codec::TranscodeType, frame::Frame,
};

pub(super) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("Failed to create decoder: {0}")]
    Create(ffmpeg_next::Error),
    #[error("Failed to decode frame: {0}")]
    Decode(ffmpeg_next::Error),
    #[error("Failed to convert frame: {0}")]
    Conversion(ffmpeg_next::Error),
    #[error("Failed to initialize scaler: {0}")]
    Scaler(ffmpeg_next::Error),
    #[error("Failed to process hardware frame: {0}")]
    HardwareInterop(String),
}

pub(crate) struct Decoder {
    dst_format: PixelFormat,
    decoder: decoder::Video,
    scaler: Option<Scaler>,
    decoding_pool: Pool,
    cached_dims: (u32, u32),
    cached_src_format: Option<format::Pixel>,
    hw_backend: Option<hw::Backend>,
}

impl Decoder {
    const BUFFER_SIZE: usize = 128000;
    const BUFFER_MAX_COUNT: usize = 4;
    const SCALING_MODE: scaling::Flags = scaling::Flags::BILINEAR;

    pub(crate) fn new(transcoding_type: TranscodeType, dst_format: PixelFormat) -> Result<Self> {
        ffmpeg::init().map_err(Error::Create)?;

        let decoder_info = transcoding_type.decoder_info();
        let codec = codec::decoder::find_by_name(decoder_info.name)
            .ok_or(Error::Create(ffmpeg::Error::DecoderNotFound))?;
        let (decoder, hw_backend) = hw::Backend::open_decoder(codec, decoder_info)?;

        Ok(Self {
            dst_format,
            decoder,
            scaler: None,
            decoding_pool: Pool::new(Self::BUFFER_SIZE, Self::BUFFER_MAX_COUNT),
            cached_dims: (0, 0),
            cached_src_format: None,
            hw_backend,
        })
    }

    pub(crate) fn decode(&mut self, packet_data: &[u8]) -> Result<Option<Arc<Frame>>> {
        let packet = Packet::borrow(packet_data);
        self.decoder.send_packet(&packet).map_err(Error::Decode)?;

        let mut decoded_frame = frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded_frame) {
            Ok(_) => {
                match self.try_decode_hw_frame(&decoded_frame) {
                    HardwareFrame::Ready(frame) => return Ok(Some(frame)),
                    HardwareFrame::Backpressured => return Ok(None),
                    HardwareFrame::Fallback => {}
                }

                self.decode_software_frame(decoded_frame).map(Some)
            }
            Err(ffmpeg::Error::Other { errno: ffmpeg::error::EAGAIN }) => Ok(None),
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(Error::Decode(e)),
        }
    }

    fn try_decode_hw_frame(&self, decoded_frame: &frame::Video) -> HardwareFrame {
        let Some(hw_backend) = self.hw_backend.as_ref() else {
            return HardwareFrame::Fallback;
        };

        match hw_backend.try_decode_frame(decoded_frame, self.dst_format) {
            Ok(hw::FrameOutput::Ready(frame)) => HardwareFrame::Ready(Arc::new(frame)),
            Ok(hw::FrameOutput::Backpressured) => HardwareFrame::Backpressured,
            Ok(hw::FrameOutput::Unsupported) => HardwareFrame::Fallback,
            Err(err) => {
                tracing::warn!(
                    "Falling back to software decode output after GPU path failed: {}",
                    err
                );
                HardwareFrame::Fallback
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
            .map_err(Error::Conversion)?;

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
            .map_err(Error::Scaler)?;

            self.scaler = Some(scaler);
            self.cached_dims = (width, height);
            self.cached_src_format = Some(src_format);
        }

        Ok(self.scaler.as_mut().expect("scaler must be initialized"))
    }

    fn copy_software_frame(&mut self, rgb_frame: &frame::Video, width: u32, height: u32) -> Frame {
        let dst_bytes_per_pixel = self.dst_format.bytes_per_pixel();
        let dst_size = (width * height * dst_bytes_per_pixel) as usize;
        let mut framebuf = self.decoding_pool.get(dst_size);

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
            Dimensions::new(width as i32, height as i32),
            None,
        )
    }
}

enum HardwareFrame {
    Fallback,
    Backpressured,
    Ready(Arc<Frame>),
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoder")
            .field("decoder", &"<Decoder>".to_owned())
            .field("decoding_pool", &self.decoding_pool)
            .finish()
    }
}

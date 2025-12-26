use std::sync::Arc;

use ffmpeg::{
    Packet, codec, decoder, frame,
    software::scaling::{self, Context as Scaler},
    sys,
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
    #[error("Failed to create HW device: {0}")]
    HWDeviceError(ffmpeg::Error),
    #[error("Hardware transfer failed: {0}")]
    HWTransferError(ffmpeg::Error),
}

pub struct FFmpegDecoder {
    decoder: decoder::Video,
    scaler: Option<Scaler>,
    decoding_pool: BufferArena,
    cached_dims: (u32, u32),
    hw_pixel_format: Option<ffmpeg::format::Pixel>,
}

impl FFmpegDecoder {
    const POOL_SIZE: usize = 128000;
    const DST_FORMAT: PixelFormat = PixelFormat::RGBA8;
    const SCALING_MODE: scaling::Flags = scaling::Flags::BILINEAR;

    pub fn new(transcoding_type: FFmpegTranscodeType) -> Result<Self> {
        ffmpeg::init().map_err(FFmpegDecoderError::CreateDecoderError)?;

        let decoder_name = transcoding_type.to_decoder_name();
        let codec = codec::decoder::find_by_name(decoder_name)
            .ok_or(FFmpegDecoderError::CreateDecoderError(ffmpeg::Error::DecoderNotFound))?;

        let mut context = codec::context::Context::new_with_codec(codec);
        context.set_flags(codec::Flags::LOW_DELAY);

        let input_format = transcoding_type.get_input_format();
        let mut opts = ffmpeg::Dictionary::new();
        let mut hw_pixel_format = None;
        if let Some(hwaccel_name) = transcoding_type.hw_accel_name() {
            opts.set("hwaccel", hwaccel_name);
            hw_pixel_format = Some(input_format);
            tracing::info!("Enabling HW Accel with option: hwaccel={}", hwaccel_name);
        }

        let decoder = context
            .decoder()
            .open_as_with(codec, opts)
            .and_then(|d| d.video())
            .map_err(FFmpegDecoderError::CreateDecoderError)?;

        Ok(Self {
            decoder,
            scaler: None,
            decoding_pool: BufferArena::init(Self::POOL_SIZE),
            cached_dims: (0, 0),
            hw_pixel_format,
        })
    }

    pub fn decode(&mut self, packet_data: &[u8]) -> Result<Option<Arc<Frame>>> {
        let packet = Packet::borrow(packet_data);
        self.decoder.send_packet(&packet).map_err(FFmpegDecoderError::DecodeError)?;

        let mut decoded_frame = frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded_frame) {
            Ok(_) => {
                // Check if the frame format matches our expected HW format.
                // If so, we must transfer the data from GPU memory to system memory.
                let final_frame = if self.hw_pixel_format == Some(decoded_frame.format()) {
                    tracing::trace!(
                        "Frame format {:?} matches HW format, attempting transfer...",
                        decoded_frame.format()
                    );
                    let mut sw_frame = frame::Video::empty();

                    unsafe {
                        let ret = sys::av_hwframe_transfer_data(
                            sw_frame.as_mut_ptr(),
                            decoded_frame.as_ptr(),
                            0,
                        );

                        if ret < 0 {
                            return Err(FFmpegDecoderError::HWTransferError(ffmpeg::Error::from(
                                ret,
                            )));
                        }
                    }
                    sw_frame
                } else {
                    decoded_frame
                };

                let width = final_frame.width();
                let height = final_frame.height();
                let format = final_frame.format();

                // Initialize or update scaler if dimensions changed
                if self.scaler.is_none() || self.cached_dims != (width, height) {
                    tracing::debug!("Initializing scaler for {}x{}", width, height);
                    let scaler = scaling::Context::get(
                        format,
                        width,
                        height,
                        Self::DST_FORMAT.to_ffmpeg_pixel_format(),
                        width,
                        height,
                        Self::SCALING_MODE,
                    )
                    .map_err(FFmpegDecoderError::ScalerError)?;

                    self.scaler = Some(scaler);
                    self.cached_dims = (width, height);
                }

                let scaler = self.scaler.as_mut().unwrap();
                let mut rgb_frame =
                    frame::Video::new(Self::DST_FORMAT.to_ffmpeg_pixel_format(), width, height);

                scaler
                    .run(&final_frame, &mut rgb_frame)
                    .map_err(FFmpegDecoderError::ConversionError)?;

                let size = (width * height * 4) as usize;
                let mut framebuf = self.decoding_pool.get(size);

                // Copy from the first plane
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
            Err(e) => Err(FFmpegDecoderError::DecodeError(e)),
        }
    }
}

impl std::fmt::Debug for FFmpegDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H264Decoder")
            .field("decoder", &"<Decoder>".to_owned())
            .field("decoding_pool", &self.decoding_pool)
            .finish()
    }
}

unsafe impl Send for FFmpegDecoder {}

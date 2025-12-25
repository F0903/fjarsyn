use ffmpeg::{
    Packet, Rational, codec, encoder, format, frame,
    software::scaling::{self, Context as Scaler},
};
use ffmpeg_next as ffmpeg;

type Result<T> = std::result::Result<T, H264EncoderError>;

#[derive(Debug, thiserror::Error)]
pub enum H264EncoderError {
    #[error("Failed to create encoder: {0}")]
    CreateEncoderError(ffmpeg::Error),
    #[error("Failed to encode frame: {0}")]
    EncodeError(ffmpeg::Error),
    #[error("Failed to convert frame: {0}")]
    ConversionError(ffmpeg::Error),
    #[error("Failed to initialize scaler: {0}")]
    ScalerError(ffmpeg::Error),
    #[error("Invalid parameters")]
    InvalidParameters,
}

/// A simple H.264 encoder using FFmpeg.
pub struct H264Encoder {
    encoder: Option<encoder::Video>,
    scaler: Option<Scaler>,
    bitrate: u32,
    target_framerate_hz: f32,
    frame_count: i64,
}

impl std::fmt::Debug for H264Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("H264Encoder"))
    }
}

impl H264Encoder {
    pub fn new(bitrate: u32, target_framerate_hz: f32) -> Result<Self> {
        ffmpeg::init().map_err(H264EncoderError::CreateEncoderError)?;

        Ok(Self { encoder: None, scaler: None, bitrate, target_framerate_hz, frame_count: 0 })
    }

    fn init_encoder(&mut self, width: i32, height: i32) -> Result<()> {
        let codec = encoder::find_by_name("libx264")
            .or_else(|| encoder::find(codec::Id::H264))
            .ok_or(H264EncoderError::CreateEncoderError(ffmpeg::Error::EncoderNotFound))?;

        tracing::info!("Using encoder: {}", codec.name());

        let mut context = codec::Context::new_with_codec(codec)
            .encoder()
            .video()
            .map_err(H264EncoderError::CreateEncoderError)?;

        context.set_width(width as u32);
        context.set_height(height as u32);
        context.set_format(format::Pixel::YUV420P);
        context.set_bit_rate(self.bitrate as usize);

        let time_base = Rational(1, self.target_framerate_hz as i32);
        context.set_time_base(time_base);
        context.set_frame_rate(Some(Rational(self.target_framerate_hz as i32, 1)));

        // Set low latency options
        context.set_gop(10); // I-frame interval
        context.set_max_b_frames(0); // No B-frames for low latency

        // Open with specific options for low latency
        let mut opts = ffmpeg::Dictionary::new();
        if codec.name() == "libx264" {
            opts.set("preset", "ultrafast");
            opts.set("tune", "zerolatency");
        }

        let encoder = context.open_with(opts).map_err(H264EncoderError::CreateEncoderError)?;

        self.encoder = Some(encoder);

        // Initialize scaler: RGBA (packed) -> YUV420P (planar)
        let scaler = scaling::Context::get(
            format::Pixel::RGBA,
            width as u32,
            height as u32,
            format::Pixel::YUV420P,
            width as u32,
            height as u32,
            scaling::Flags::BILINEAR,
        )
        .map_err(H264EncoderError::ScalerError)?;

        self.scaler = Some(scaler);

        Ok(())
    }

    /// Encodes a raw RGBA8 bitmap into a list of H.264 NAL units (as packets).
    pub fn encode(&mut self, bitmap: &[u8], width: i32, height: i32) -> Result<Vec<Vec<u8>>> {
        if self.encoder.is_none() {
            self.init_encoder(width, height)?;
        }

        // Check if resolution changed (simple check, assume re-init if needed, but for now just error or ignore)
        // In a real scenario, we might want to re-init if dimensions change.
        if let Some(enc) = &self.encoder {
            if enc.width() != width as u32 || enc.height() != height as u32 {
                // Re-init
                self.init_encoder(width, height)?;
            }
        }

        let encoder = self.encoder.as_mut().unwrap();
        let scaler = self.scaler.as_mut().unwrap();

        // 1. Create Input Frame (RGBA)
        let mut input_frame = frame::Video::new(format::Pixel::RGBA, width as u32, height as u32);
        input_frame.data_mut(0).copy_from_slice(bitmap);

        // 2. Convert to YUV420P
        let mut yuv_frame = frame::Video::new(format::Pixel::YUV420P, width as u32, height as u32);
        scaler.run(&input_frame, &mut yuv_frame).map_err(H264EncoderError::ConversionError)?;

        yuv_frame.set_pts(Some(self.frame_count));
        self.frame_count += 1;

        // 3. Send to Encoder
        encoder.send_frame(&yuv_frame).map_err(H264EncoderError::EncodeError)?;

        // 4. Receive Packets
        let mut nal_units = Vec::new();
        let mut packet = Packet::empty();

        while encoder.receive_packet(&mut packet).is_ok() {
            // We treat the packet data as a "NAL unit" blob.
            // FFmpeg x264 encoder typically outputs Annex B with start codes.
            if let Some(data) = packet.data() {
                nal_units.push(data.to_vec());
            }
        }

        Ok(nal_units)
    }
}

unsafe impl Send for H264Encoder {}

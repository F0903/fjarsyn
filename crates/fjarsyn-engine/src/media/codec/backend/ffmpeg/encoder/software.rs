use ffmpeg::util::format;
use ffmpeg_next as ffmpeg;

use super::{Encoder, Error, Result};
use crate::media::frame::Frame;

impl Encoder {
    pub(super) fn encode_software(
        &mut self,
        frame: &Frame,
        width: i32,
        height: i32,
        dst_w: u32,
        dst_h: u32,
        dst_format: format::Pixel,
    ) -> Result<()> {
        let encoder = self.encoder.as_mut().unwrap();
        let mut input_frame = ffmpeg::frame::Video::empty();

        let pixels =
            frame.software_pixels().ok_or(Error::Conversion(ffmpeg::Error::InvalidData))?;

        let (input_format, stride) = software_input_layout(frame, width);
        input_frame.set_format(input_format);
        input_frame.set_width(width as u32);
        input_frame.set_height(height as u32);

        unsafe {
            let ptr = input_frame.as_mut_ptr();

            (*ptr).data[0] = pixels.as_ptr() as *mut u8;
            (*ptr).linesize[0] = stride;
            (*ptr).extended_data = (*ptr).data.as_mut_ptr();
        }

        let mut dst_frame = ffmpeg::frame::Video::new(dst_format, dst_w, dst_h);
        let scaler = self.scaler.as_mut().unwrap();

        let scale_result = scaler.run(&input_frame, &mut dst_frame);

        unsafe {
            let ptr = input_frame.as_mut_ptr();
            (*ptr).data[0] = std::ptr::null_mut();
            (*ptr).linesize[0] = 0;
            (*ptr).extended_data = std::ptr::null_mut();
        }

        if let Err(err) = scale_result {
            return Err(Error::Conversion(err));
        }

        dst_frame.set_pts(Some(self.frame_count));
        self.frame_count += 1;

        encoder.send_frame(&dst_frame).map_err(Error::Encode)
    }
}

fn software_input_layout(frame: &Frame, width: i32) -> (format::Pixel, i32) {
    (frame.format.to_ffmpeg_pixel_format(), width * frame.format.bytes_per_pixel() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{Dimensions, PixelFormat, buffer_pool::Pool};

    #[test]
    fn software_encoder_uses_the_frames_converted_pixel_format() {
        let pool = Pool::new(4, 1);
        let mut pixels = pool.get(4);
        pixels.copy_from_slice(&[10, 20, 30, 255]);
        let frame = Frame::new_software(pixels, PixelFormat::BGRA8, Dimensions::new(1, 1), None);

        assert_eq!(frame.format, PixelFormat::RGBA8);
        assert_eq!(software_input_layout(&frame, 1), (format::Pixel::RGBA, 4));
    }
}

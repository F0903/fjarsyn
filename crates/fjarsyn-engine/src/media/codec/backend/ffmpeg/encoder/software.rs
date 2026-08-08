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

        input_frame.set_format(self.input_format.to_ffmpeg_pixel_format());
        input_frame.set_width(width as u32);
        input_frame.set_height(height as u32);

        unsafe {
            let ptr = input_frame.as_mut_ptr();
            let stride = width * self.input_format.bytes_per_pixel() as i32;

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

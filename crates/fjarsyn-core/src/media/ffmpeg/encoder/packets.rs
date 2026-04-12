use ffmpeg_next::Packet;

use super::{FFmpegEncoder, Result};

impl FFmpegEncoder {
    pub(super) fn collect_nal_units(&mut self) -> Result<Vec<Vec<u8>>> {
        let encoder = self.encoder.as_mut().unwrap();
        let mut nal_units = Vec::new();
        let mut packet = Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            if let Some(data) = packet.data() {
                nal_units.push(data.as_ref().to_vec());
            }
        }
        Ok(nal_units)
    }
}

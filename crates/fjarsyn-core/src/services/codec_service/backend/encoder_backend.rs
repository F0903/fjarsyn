use crate::media::frame::Frame;

pub(in crate::services::codec_service) trait EncoderBackend {
    fn encode(&mut self, frame: &Frame) -> Result<Vec<Vec<u8>>, String>;
}

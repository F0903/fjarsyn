use std::sync::Arc;

use crate::media::frame::Frame;

pub(in crate::services::codec_service) trait DecoderBackend {
    fn decode(&mut self, packet: &[u8]) -> Result<Option<Arc<Frame>>, String>;
}

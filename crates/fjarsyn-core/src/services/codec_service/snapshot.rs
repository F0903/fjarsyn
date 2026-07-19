use crate::services::codec_service::{CodecDirection, CodecDirectionState};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub encode: CodecDirectionState,
    pub decode: CodecDirectionState,
}

impl Snapshot {
    pub fn direction(&self, direction: CodecDirection) -> &CodecDirectionState {
        match direction {
            CodecDirection::Encode => &self.encode,
            CodecDirection::Decode => &self.decode,
        }
    }
}

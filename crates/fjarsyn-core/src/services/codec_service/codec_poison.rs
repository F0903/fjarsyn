use std::fmt;

use crate::services::codec_service::{CodecDirection, CodecOperation, CodecPoisonReason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecPoison {
    pub direction: CodecDirection,
    pub operation: CodecOperation,
    pub reason: CodecPoisonReason,
}

impl fmt::Display for CodecPoison {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let direction = match self.direction {
            CodecDirection::Encode => "video encoding",
            CodecDirection::Decode => "video decoding",
        };
        let cause = match self.reason {
            CodecPoisonReason::DeadlineExceeded => "a codec operation exceeded its deadline",
            CodecPoisonReason::WorkerTerminated => "the codec worker terminated unexpectedly",
        };
        write!(formatter, "{direction} is unavailable until Fjarsyn restarts: {cause}")
    }
}

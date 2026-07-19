use crate::services::codec_service::CodecPoison;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CodecWorkerError {
    #[error("codec failed: {0}")]
    Codec(String),
    #[error("{0}")]
    RestartRequired(CodecPoison),
    #[error("codec service stopped")]
    ServiceStopped,
}

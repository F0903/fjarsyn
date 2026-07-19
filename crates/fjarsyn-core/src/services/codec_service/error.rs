use crate::services::codec_service::CodecPoison;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("codec service is shutting down")]
    ShuttingDown,
    #[error("{0}")]
    RestartRequired(CodecPoison),
    #[error("failed to spawn codec worker: {0}")]
    WorkerSpawn(String),
    #[error("failed to initialize codec: {0}")]
    Codec(String),
}

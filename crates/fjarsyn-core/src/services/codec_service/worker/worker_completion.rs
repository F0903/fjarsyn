//! Terminal state observed by codec handles and output ports.

use crate::services::codec_service::CodecWorkerError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::services::codec_service) enum WorkerCompletion {
    Running,
    Finished(Result<(), CodecWorkerError>),
}

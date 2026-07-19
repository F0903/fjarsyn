//! Commands sent by the service registry to a codec supervisor.

use crate::services::codec_service::CodecPoison;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::services::codec_service) enum WorkerDirective {
    Run,
    Stop,
    Poisoned(CodecPoison),
    ServiceShutdown,
}

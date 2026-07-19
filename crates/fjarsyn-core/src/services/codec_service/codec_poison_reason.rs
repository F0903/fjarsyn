#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecPoisonReason {
    DeadlineExceeded,
    WorkerTerminated,
}

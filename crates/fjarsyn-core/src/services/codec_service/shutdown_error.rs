#[derive(Debug, thiserror::Error)]
#[error("{remaining_workers} codec worker(s) did not stop before the shutdown deadline")]
pub struct ShutdownError {
    pub remaining_workers: usize,
}

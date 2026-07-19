#[derive(Debug, Clone)]
pub enum LifecycleMessage {
    RetryStartup,
    RestartRequested,
    ExitRequested,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("signaling port cannot be zero")]
    InvalidSignalingPort,
    #[error("mDNS instance name cannot be empty")]
    InvalidInstanceName,
    #[error("mDNS hostname cannot be empty")]
    InvalidHostname,
    #[error("presence limit {name} must be greater than zero")]
    InvalidLimit { name: &'static str },
    #[error("presence shutdown timeout must be greater than zero")]
    InvalidShutdownTimeout,
    #[error("failed to create the mDNS daemon: {0}")]
    CreateDaemon(#[source] mdns_sd::Error),
    #[error("failed to create the local mDNS advertisement: {0}")]
    CreateAdvertisement(#[source] mdns_sd::Error),
    #[error("failed to advertise local presence: {0}")]
    Advertise(#[source] mdns_sd::Error),
    #[error("failed to browse for nearby peers: {0}")]
    Browse(#[source] mdns_sd::Error),
    #[error("presence observation stream closed unexpectedly")]
    ObservationStreamClosed,
    #[error("failed to stop browsing for nearby peers: {0}")]
    StopBrowse(#[source] mdns_sd::Error),
    #[error("failed to withdraw the local presence advertisement: {0}")]
    WithdrawAdvertisement(#[source] mdns_sd::Error),
    #[error("failed to stop the mDNS daemon: {0}")]
    ShutdownDaemon(#[source] mdns_sd::Error),
    #[error("mDNS cleanup did not acknowledge {operation}")]
    CleanupNotAcknowledged { operation: &'static str },
    #[error("presence worker task failed: {0}")]
    WorkerJoin(#[source] tokio::task::JoinError),
    #[error("presence worker did not stop before its shutdown deadline")]
    ShutdownTimeout,
}

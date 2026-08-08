#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("screen-share service is unavailable")]
    ServiceUnavailable,
    #[error("screen-share service stopped before replying")]
    ResponseDropped,
    #[error(transparent)]
    PeerSession(#[from] crate::peer_session::Error),
    #[error("{0}")]
    Operation(String),
}

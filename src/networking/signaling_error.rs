#[derive(Debug, thiserror::Error)]
pub enum SignalingError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("Deserialize error: {0}")]
    DeserializeError(#[from] serde_json::Error),
}

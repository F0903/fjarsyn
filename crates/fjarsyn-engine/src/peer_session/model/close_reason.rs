#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    LocalDisconnect,
    RemoteDisconnect,
    Rejected { reason: String },
    Cancelled,
    SignalingLost,
    ConnectionFailed { reason: String },
    ProtocolViolation { reason: String },
    TrustRevoked,
    ServiceShutdown,
}

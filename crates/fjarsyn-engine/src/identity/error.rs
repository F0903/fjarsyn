#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid base64 {kind}: {source}")]
    InvalidBase64 { kind: &'static str, source: base64::DecodeError },
    #[error("invalid {kind} length: expected {expected} bytes, got {actual}")]
    InvalidLength { kind: &'static str, expected: usize, actual: usize },
    #[error("stored public key does not match the stored private key")]
    PublicKeyMismatch,
    #[error("invalid public key: {0}")]
    InvalidPublicKey(ed25519_dalek::SignatureError),
    #[error("weak Ed25519 public keys are not accepted")]
    WeakPublicKey,
}

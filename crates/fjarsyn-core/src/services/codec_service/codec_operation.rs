#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecOperation {
    Initialize,
    Encode,
    Decode,
    Shutdown,
}

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub nal_units: Vec<Vec<u8>>,
    pub duration: Duration,
}

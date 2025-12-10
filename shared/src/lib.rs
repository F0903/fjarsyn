use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SignalingType {
    Offer,
    Answer,
    Candidate,
}

// Our signaling message format
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignalingMessage {
    pub signalling_type: SignalingType,
    pub data: String,
}

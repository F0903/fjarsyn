use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SignalingType {
    Offer,
    Answer,
    Candidate,
    Decline,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignalingMessage {
    pub from: String, // Peer ID of the sender
    pub sig_type: SignalingType,
    pub data: String,
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SignalingType {
    Offer,
    Answer,
    Candidate,
    Identity,
}

// Our signaling message format
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignalingMessage {
    pub to: String,
    pub from: String,
    pub sig_type: SignalingType,
    pub data: String,
}

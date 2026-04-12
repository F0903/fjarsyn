use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SignalingType {
    Offer,
    Answer,
    Candidate,
    Decline,
    ChatMessage,
    ChatReceipt,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SignalingMessage {
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub sig_type: SignalingType,
    pub data: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatMessagePayload {
    pub message_id: String,
    pub body: String,
    pub sent_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatReceiptPayload {
    pub message_id: String,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

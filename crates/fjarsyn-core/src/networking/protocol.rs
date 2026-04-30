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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SignalingMessage {
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub sig_type: SignalingType,
    pub data: String,
}

impl SignalingMessage {
    pub fn targets_peer(&self, peer_id: &str) -> bool {
        match self.to.as_deref() {
            Some(target_peer_id) => target_peer_id == peer_id,
            None => true,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn message_to(target: Option<&str>) -> SignalingMessage {
        SignalingMessage {
            from: "peer-a".into(),
            to: target.map(str::to_string),
            sig_type: SignalingType::ChatReceipt,
            data: String::new(),
        }
    }

    #[test]
    fn broadcast_message_targets_any_peer() {
        assert!(message_to(None).targets_peer("local-peer"));
    }

    #[test]
    fn addressed_message_targets_matching_peer() {
        assert!(message_to(Some("local-peer")).targets_peer("local-peer"));
    }

    #[test]
    fn addressed_message_does_not_target_other_peer() {
        assert!(!message_to(Some("other-peer")).targets_peer("local-peer"));
    }
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::validate_data_version;
use crate::peer_session::{Error, MessageId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(in crate::peer_session) enum MessagingMessage {
    Chat { version: u8, message_id: MessageId, body: String, sent_at: DateTime<Utc> },
    Receipt { version: u8, message_id: MessageId, received_at: DateTime<Utc> },
}

impl MessagingMessage {
    pub(in crate::peer_session) fn validate(&self, max_body_bytes: usize) -> Result<(), Error> {
        let version = match self {
            Self::Chat { version, body, .. } => {
                if body.trim().is_empty() {
                    return Err(Error::EmptyMessage);
                }
                if body.len() > max_body_bytes {
                    return Err(Error::MessageTooLarge { max: max_body_bytes });
                }
                *version
            }
            Self::Receipt { version, .. } => *version,
        };
        validate_data_version(version)
    }
}

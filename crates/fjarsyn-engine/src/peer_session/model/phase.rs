use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Requesting,
    Incoming,
    Negotiating,
    Connected,
    Reconnecting,
    Disconnecting,
}

impl Phase {
    pub(in crate::peer_session) fn name(self) -> &'static str {
        match self {
            Self::Requesting => "requesting",
            Self::Incoming => "incoming",
            Self::Negotiating => "negotiating",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Disconnecting => "disconnecting",
        }
    }
}

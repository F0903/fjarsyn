const CONTROL_CHANNEL_LABEL: &str = "fjarsyn-control-v2";
const MESSAGING_CHANNEL_LABEL: &str = "fjarsyn-messaging-v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::peer_session) enum ChannelKind {
    Control,
    Messaging,
}

impl ChannelKind {
    pub(super) fn from_label(label: &str) -> Option<Self> {
        match label {
            CONTROL_CHANNEL_LABEL => Some(Self::Control),
            MESSAGING_CHANNEL_LABEL => Some(Self::Messaging),
            _ => None,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Control => CONTROL_CHANNEL_LABEL,
            Self::Messaging => MESSAGING_CHANNEL_LABEL,
        }
    }
}

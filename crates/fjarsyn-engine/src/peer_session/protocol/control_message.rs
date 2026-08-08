use serde::{Deserialize, Serialize};

use super::validate_data_version;
use crate::peer_session::{Error, ShareEpoch, ShareId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(in crate::peer_session) enum ControlMessage {
    ShareStarted { version: u8, share_id: ShareId, epoch: ShareEpoch },
    ShareStopped { version: u8, share_id: ShareId, epoch: ShareEpoch },
    Disconnect { version: u8 },
}

impl ControlMessage {
    pub(in crate::peer_session) fn validate(&self) -> Result<(), Error> {
        let version = match self {
            Self::ShareStarted { version, .. }
            | Self::ShareStopped { version, .. }
            | Self::Disconnect { version } => *version,
        };
        validate_data_version(version)?;
        match self {
            Self::ShareStarted { epoch, .. } | Self::ShareStopped { epoch, .. } => {
                epoch.require_valid()
            }
            Self::Disconnect { .. } => Ok(()),
        }
    }
}

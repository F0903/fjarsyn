use super::{ShareEpoch, ShareId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LocalShareState {
    #[default]
    Inactive,
    Active {
        share_id: ShareId,
        epoch: ShareEpoch,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RemoteShareState {
    #[default]
    Inactive,
    Active {
        share_id: ShareId,
        epoch: ShareEpoch,
    },
}

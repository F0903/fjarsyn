use crate::peer_session::{SessionId, ShareEpoch, ShareId};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LocalState {
    #[default]
    Inactive,
    Selecting,
    Starting,
    Active,
    Stopping,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RemoteState {
    #[default]
    Inactive,
    Starting,
    Active,
    Failed(String),
}

/// Exact identity of one authenticated screen share and its epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ShareBinding {
    pub(super) share_id: ShareId,
    pub(super) epoch: ShareEpoch,
}

impl ShareBinding {
    pub const fn new(share_id: ShareId, epoch: ShareEpoch) -> Self {
        Self { share_id, epoch }
    }

    pub const fn share_id(self) -> ShareId {
        self.share_id
    }

    pub const fn epoch(self) -> ShareEpoch {
        self.epoch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct LocalShareBinding {
    pub(super) session_id: SessionId,
    pub(super) share_id: ShareId,
    pub(super) epoch: ShareEpoch,
}

impl LocalShareBinding {
    pub(super) const fn new(session_id: SessionId, share_id: ShareId, epoch: ShareEpoch) -> Self {
        Self { session_id, share_id, epoch }
    }

    pub(super) const fn session_id(self) -> SessionId {
        self.session_id
    }

    pub(super) const fn share_id(self) -> ShareId {
        self.share_id
    }

    pub(super) const fn media(self) -> ShareBinding {
        ShareBinding::new(self.share_id, self.epoch)
    }
}

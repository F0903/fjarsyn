use tokio::sync::oneshot;

use super::Selection;
use crate::{media::capture::PlatformItem, peer_session::SessionId};

pub(super) enum Command {
    BeginSelection {
        selection: Selection,
        reply: oneshot::Sender<Result<(), String>>,
    },
    CancelSelection {
        selection: Selection,
        reply: oneshot::Sender<Result<(), String>>,
    },
    FailSelection {
        selection: Selection,
        reason: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    StartScreenShare {
        selection: Selection,
        item: PlatformItem,
        reply: oneshot::Sender<Result<(), String>>,
    },
    StopScreenShare {
        session_id: SessionId,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

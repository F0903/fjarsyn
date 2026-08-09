use std::sync::{Arc, Mutex};

use tokio::{
    sync::{broadcast, mpsc, watch},
    time::Instant,
};

use super::{ActorInstanceId, Command, restart::Attachment};
use crate::peer_session::{
    EncodedVideoSink, Error, LocalShareState, RemoteVideoSource, SessionId, SessionState,
    ShareEpoch, ShareId,
    media::{OutboundVideoSample, RemoteVideoSample},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Control {
    Fail(String),
    TrustRevoked { deadline: Instant },
    Shutdown { deadline: Instant },
}

#[derive(Debug, Clone)]
pub(in crate::peer_session) struct Handle {
    pub session_id: SessionId,
    pub instance_id: ActorInstanceId,
    pub(super) command_tx: mpsc::Sender<Command>,
    pub(super) restart_tx: mpsc::Sender<Attachment>,
    pub(super) snapshot_rx: watch::Receiver<SessionState>,
    pub(super) encoded_video_tx: mpsc::Sender<OutboundVideoSample>,
    pub(super) active_video_rx: watch::Receiver<Option<(ShareId, ShareEpoch)>>,
    pub(super) remote_video_tx: broadcast::Sender<RemoteVideoSample>,
    pub(super) initial_remote_video_rx: Arc<Mutex<Option<broadcast::Receiver<RemoteVideoSample>>>>,
    pub(super) fatal_tx: watch::Sender<Option<Control>>,
}

impl Handle {
    pub(in crate::peer_session) fn command_tx(&self) -> mpsc::Sender<Command> {
        self.command_tx.clone()
    }

    pub(in crate::peer_session) fn try_attach_restart(
        &self,
        attachment: Attachment,
    ) -> Result<(), Box<Attachment>> {
        self.restart_tx.try_send(attachment).map_err(|error| Box::new(error.into_inner()))
    }

    pub(in crate::peer_session) fn snapshot(&self) -> SessionState {
        self.snapshot_rx.borrow().clone()
    }

    pub(in crate::peer_session) fn encoded_video_sink(
        &self,
        share_id: ShareId,
    ) -> Result<EncodedVideoSink, Error> {
        match self.snapshot_rx.borrow().local_share {
            LocalShareState::Active { share_id: active, epoch } if active == share_id => {
                Ok(EncodedVideoSink::new(
                    self.session_id,
                    share_id,
                    epoch,
                    self.encoded_video_tx.clone(),
                    self.active_video_rx.clone(),
                ))
            }
            _ => Err(Error::ShareMismatch(share_id)),
        }
    }

    pub(in crate::peer_session) fn remote_video_source(&self) -> RemoteVideoSource {
        let receiver = self
            .initial_remote_video_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .unwrap_or_else(|| self.remote_video_tx.subscribe());
        RemoteVideoSource::new(receiver)
    }

    pub(in crate::peer_session) fn fail(&self, reason: impl Into<String>) {
        self.fatal_tx.send_replace(Some(Control::Fail(reason.into())));
    }

    pub(in crate::peer_session) fn shutdown(&self, deadline: Instant) {
        self.fatal_tx.send_replace(Some(Control::Shutdown { deadline }));
    }

    pub(in crate::peer_session) fn revoke_trust(&self, deadline: Instant) {
        self.fatal_tx.send_replace(Some(Control::TrustRevoked { deadline }));
    }
}

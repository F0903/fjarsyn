use tokio::sync::{mpsc, watch};

use super::EncodedVideoSample;
use crate::peer_session::{Error, SessionId, ShareEpoch, ShareId};

/// One outbound encoded sample bound to the share instance that produced it.
///
/// This binding is retained across every sender-side asynchronous queue. It
/// must never be inferred from the actor's current share when a queued sample is
/// eventually consumed, because a newer share may already be active then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::peer_session) struct OutboundVideoSample {
    pub share_id: ShareId,
    pub epoch: ShareEpoch,
    pub sample: EncodedVideoSample,
}

/// Bounded capability for submitting encoded video to one live session.
#[derive(Debug, Clone)]
pub(crate) struct EncodedVideoSink {
    session_id: SessionId,
    share_id: ShareId,
    epoch: ShareEpoch,
    tx: mpsc::Sender<OutboundVideoSample>,
    active_share: watch::Receiver<Option<(ShareId, ShareEpoch)>>,
}

impl EncodedVideoSink {
    pub(in crate::peer_session) fn new(
        session_id: SessionId,
        share_id: ShareId,
        epoch: ShareEpoch,
        tx: mpsc::Sender<OutboundVideoSample>,
        active_share: watch::Receiver<Option<(ShareId, ShareEpoch)>>,
    ) -> Self {
        Self { session_id, share_id, epoch, tx, active_share }
    }

    pub(crate) fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub(crate) fn share_id(&self) -> ShareId {
        self.share_id
    }

    pub(crate) fn epoch(&self) -> ShareEpoch {
        self.epoch
    }

    pub(crate) async fn send(&self, sample: EncodedVideoSample) -> Result<(), Error> {
        let binding = (self.share_id, self.epoch);
        let mut active_share = self.active_share.clone();
        let is_active = *active_share.borrow_and_update() == Some(binding);
        if !is_active {
            return Err(Error::MediaClosed);
        }
        let tagged = OutboundVideoSample { share_id: self.share_id, epoch: self.epoch, sample };
        tokio::select! {
            biased;
            _ = active_share.changed() => Err(Error::MediaClosed),
            result = self.tx.send(tagged) => result.map_err(|_| Error::MediaClosed),
        }
    }

    #[cfg(test)]
    pub(crate) fn try_send(&self, sample: EncodedVideoSample) -> Result<(), Error> {
        if *self.active_share.borrow() != Some((self.share_id, self.epoch)) {
            return Err(Error::MediaClosed);
        }
        self.tx
            .try_send(OutboundVideoSample { share_id: self.share_id, epoch: self.epoch, sample })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Closed(_) => Error::MediaClosed,
                mpsc::error::TrySendError::Full(_) => {
                    Error::Protocol("encoded-video queue is full".into())
                }
            })
    }
}

pub(in crate::peer_session) fn encoded_video_channel(
    capacity: usize,
) -> (mpsc::Sender<OutboundVideoSample>, mpsc::Receiver<OutboundVideoSample>) {
    mpsc::channel(capacity.max(1))
}

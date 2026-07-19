use std::time::Duration;

use bytes::Bytes;
use tokio::sync::{broadcast, mpsc, watch};

use super::{PeerSessionError, SessionId, ShareEpoch, ShareId};

/// One encoded H.264 access unit and its intended media duration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedVideoSample {
    pub data: Bytes,
    pub duration: Duration,
}

impl EncodedVideoSample {
    pub fn new(data: impl Into<Bytes>, duration: Duration) -> Self {
        Self { data: data.into(), duration }
    }
}

/// One outbound encoded sample bound to the share instance that produced it.
///
/// This binding is retained across every sender-side asynchronous queue. It
/// must never be inferred from the actor's current share when a queued sample is
/// eventually consumed, because a newer share may already be active then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutboundVideoSample {
    pub share_id: ShareId,
    pub epoch: ShareEpoch,
    pub sample: EncodedVideoSample,
}

/// One depacketized remote sample carrying its authenticated RTP media epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteVideoSample {
    pub epoch: ShareEpoch,
    pub sample: EncodedVideoSample,
}

/// Bounded capability for submitting encoded video to one live session.
#[derive(Debug, Clone)]
pub struct EncodedVideoSink {
    session_id: SessionId,
    share_id: ShareId,
    epoch: ShareEpoch,
    tx: mpsc::Sender<OutboundVideoSample>,
    active_share: watch::Receiver<Option<(ShareId, ShareEpoch)>>,
}

impl EncodedVideoSink {
    pub(crate) fn new(
        session_id: SessionId,
        share_id: ShareId,
        epoch: ShareEpoch,
        tx: mpsc::Sender<OutboundVideoSample>,
        active_share: watch::Receiver<Option<(ShareId, ShareEpoch)>>,
    ) -> Self {
        Self { session_id, share_id, epoch, tx, active_share }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn share_id(&self) -> ShareId {
        self.share_id
    }

    pub fn epoch(&self) -> ShareEpoch {
        self.epoch
    }

    pub async fn send(&self, sample: EncodedVideoSample) -> Result<(), PeerSessionError> {
        let binding = (self.share_id, self.epoch);
        let mut active_share = self.active_share.clone();
        let is_active = *active_share.borrow_and_update() == Some(binding);
        if !is_active {
            return Err(PeerSessionError::MediaClosed);
        }
        let tagged = OutboundVideoSample { share_id: self.share_id, epoch: self.epoch, sample };
        tokio::select! {
            biased;
            _ = active_share.changed() => Err(PeerSessionError::MediaClosed),
            result = self.tx.send(tagged) => result.map_err(|_| PeerSessionError::MediaClosed),
        }
    }

    pub fn try_send(&self, sample: EncodedVideoSample) -> Result<(), PeerSessionError> {
        if *self.active_share.borrow() != Some((self.share_id, self.epoch)) {
            return Err(PeerSessionError::MediaClosed);
        }
        self.tx
            .try_send(OutboundVideoSample { share_id: self.share_id, epoch: self.epoch, sample })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Closed(_) => PeerSessionError::MediaClosed,
                mpsc::error::TrySendError::Full(_) => {
                    PeerSessionError::Protocol("encoded-video queue is full".into())
                }
            })
    }
}

/// Result of reading the session-wide remote media stream for one share epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteVideoRead {
    Sample(EncodedVideoSample),
    /// Media for a later share arrived before its ordered control event was
    /// projected. The first later sample remains retained in the source so the
    /// next decoder can consume its SPS/PPS/IDR boundary.
    EpochAdvanced {
        next_epoch: ShareEpoch,
    },
}

/// Read-only stream of encoded video received from one live session.
#[derive(Debug)]
pub struct RemoteVideoSource {
    session_id: SessionId,
    rx: broadcast::Receiver<RemoteVideoSample>,
    pending: Option<RemoteVideoSample>,
}

impl RemoteVideoSource {
    pub(crate) fn new(session_id: SessionId, rx: broadcast::Receiver<RemoteVideoSample>) -> Self {
        Self { session_id, rx, pending: None }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the next encoded sample belonging to exactly `epoch`.
    ///
    /// Older samples are consumed and discarded. The first newer sample is
    /// retained and reported as an epoch advance so an old decoder cannot
    /// consume the new share's keyframe before its control event is projected.
    pub async fn recv_for(
        &mut self,
        epoch: ShareEpoch,
    ) -> Result<RemoteVideoRead, PeerSessionError> {
        epoch.require_valid()?;
        loop {
            let tagged = match self.pending.take() {
                Some(pending) => pending,
                None => match self.rx.recv().await {
                    Ok(tagged) => tagged,
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(PeerSessionError::MediaClosed);
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        return Err(PeerSessionError::RemoteVideoLagged { skipped });
                    }
                },
            };
            match tagged.epoch.cmp(&epoch) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => {
                    return Ok(RemoteVideoRead::Sample(tagged.sample));
                }
                std::cmp::Ordering::Greater => {
                    let next_epoch = tagged.epoch;
                    self.pending = Some(tagged);
                    return Ok(RemoteVideoRead::EpochAdvanced { next_epoch });
                }
            }
        }
    }
}

pub(crate) fn encoded_video_channel(
    capacity: usize,
) -> (mpsc::Sender<OutboundVideoSample>, mpsc::Receiver<OutboundVideoSample>) {
    mpsc::channel(capacity.max(1))
}

pub(crate) fn remote_video_channel(
    capacity: usize,
) -> (broadcast::Sender<RemoteVideoSample>, broadcast::Receiver<RemoteVideoSample>) {
    broadcast::channel(capacity.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(data: &'static [u8]) -> EncodedVideoSample {
        EncodedVideoSample::new(Bytes::from_static(data), Duration::from_millis(16))
    }

    #[tokio::test]
    async fn outbound_sink_immutably_binds_share_and_epoch_at_enqueue() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let epoch = ShareEpoch::from_value(7);
        let (tx, mut rx) = encoded_video_channel(2);
        let (_active_tx, active_rx) = watch::channel(Some((share_id, epoch)));
        let sink = EncodedVideoSink::new(session_id, share_id, epoch, tx, active_rx);

        sink.send(sample(b"frame")).await.unwrap();
        let tagged = rx.recv().await.unwrap();
        assert_eq!(tagged.share_id, share_id);
        assert_eq!(tagged.epoch, epoch);
        assert_eq!(tagged.sample, sample(b"frame"));
    }

    #[tokio::test]
    async fn revoked_sink_cannot_keep_a_new_share_backpressured() {
        let session_id = SessionId::new();
        let share_a = ShareId::new();
        let share_b = ShareId::new();
        let epoch_a = ShareEpoch::FIRST;
        let epoch_b = epoch_a.next().unwrap();
        let (tx, mut rx) = encoded_video_channel(1);
        let (active_tx, active_rx) = watch::channel(Some((share_a, epoch_a)));
        let sink_a =
            EncodedVideoSink::new(session_id, share_a, epoch_a, tx.clone(), active_rx.clone());

        sink_a.send(sample(b"a-queued")).await.unwrap();
        let blocked_a = tokio::spawn({
            let sink_a = sink_a.clone();
            async move { sink_a.send(sample(b"a-blocked")).await }
        });
        tokio::task::yield_now().await;

        active_tx.send_replace(Some((share_b, epoch_b)));
        assert_eq!(blocked_a.await.unwrap(), Err(PeerSessionError::MediaClosed));
        assert_eq!(sink_a.try_send(sample(b"a-stale")), Err(PeerSessionError::MediaClosed));

        assert_eq!(rx.recv().await.unwrap().sample, sample(b"a-queued"));
        let sink_b = EncodedVideoSink::new(session_id, share_b, epoch_b, tx, active_rx);
        sink_b.try_send(sample(b"b-current")).unwrap();
        assert_eq!(rx.recv().await.unwrap().sample, sample(b"b-current"));
    }

    #[tokio::test]
    async fn remote_source_preserves_future_epoch_and_discards_delayed_tail() {
        const A_CURRENT: &[u8] = &[0, 0, 0, 1, 0x65, 0xa1];
        const B_BOOTSTRAP: &[u8] =
            &[0, 0, 0, 1, 0x67, 0xb1, 0, 0, 0, 1, 0x68, 0xb1, 0, 0, 0, 1, 0x65, 0xb1];
        const A_DELAYED_BOOTSTRAP: &[u8] =
            &[0, 0, 0, 1, 0x67, 0xaf, 0, 0, 0, 1, 0x68, 0xaf, 0, 0, 0, 1, 0x65, 0xaf];
        const B_NEXT: &[u8] = &[0, 0, 0, 1, 0x61, 0xb2];
        let session_id = SessionId::new();
        let epoch_a = ShareEpoch::FIRST;
        let epoch_b = epoch_a.next().unwrap();
        let (tx, rx) = remote_video_channel(8);
        let mut source = RemoteVideoSource::new(session_id, rx);

        tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(A_CURRENT) }).unwrap();
        tx.send(RemoteVideoSample { epoch: epoch_b, sample: sample(B_BOOTSTRAP) }).unwrap();
        tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(A_DELAYED_BOOTSTRAP) }).unwrap();
        tx.send(RemoteVideoSample { epoch: epoch_b, sample: sample(B_NEXT) }).unwrap();

        assert_eq!(
            source.recv_for(epoch_a).await.unwrap(),
            RemoteVideoRead::Sample(sample(A_CURRENT))
        );
        assert_eq!(
            source.recv_for(epoch_a).await.unwrap(),
            RemoteVideoRead::EpochAdvanced { next_epoch: epoch_b }
        );
        assert_eq!(
            source.recv_for(epoch_b).await.unwrap(),
            RemoteVideoRead::Sample(sample(B_BOOTSTRAP))
        );
        assert_eq!(
            source.recv_for(epoch_b).await.unwrap(),
            RemoteVideoRead::Sample(sample(B_NEXT))
        );
    }

    #[tokio::test]
    async fn remote_source_recovers_from_lag_without_losing_future_epoch_handoff() {
        let session_id = SessionId::new();
        let epoch_a = ShareEpoch::FIRST;
        let epoch_b = epoch_a.next().unwrap();
        let (tx, rx) = remote_video_channel(2);
        let mut source = RemoteVideoSource::new(session_id, rx);

        tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(b"a-old-1") }).unwrap();
        tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(b"a-old-2") }).unwrap();
        tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(b"a-retained") }).unwrap();
        tx.send(RemoteVideoSample { epoch: epoch_b, sample: sample(b"b-bootstrap") }).unwrap();

        assert!(matches!(
            source.recv_for(epoch_a).await,
            Err(PeerSessionError::RemoteVideoLagged { skipped }) if skipped >= 1
        ));
        assert_eq!(
            source.recv_for(epoch_a).await.unwrap(),
            RemoteVideoRead::Sample(sample(b"a-retained"))
        );
        assert_eq!(
            source.recv_for(epoch_a).await.unwrap(),
            RemoteVideoRead::EpochAdvanced { next_epoch: epoch_b }
        );
        assert_eq!(
            source.recv_for(epoch_b).await.unwrap(),
            RemoteVideoRead::Sample(sample(b"b-bootstrap"))
        );
    }
}

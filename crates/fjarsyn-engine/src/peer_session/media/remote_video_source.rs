use tokio::sync::broadcast;

use super::EncodedVideoSample;
use crate::peer_session::{Error, ShareEpoch};

/// Result of reading the session-wide remote media stream for one share epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemoteVideoRead {
    /// The next current-epoch access unit. Its discontinuity marker combines
    /// RTP depacketization loss with any broadcast-ring overwrite.
    Sample(EncodedVideoSample),
    /// Media for a later share arrived before its ordered control event was
    /// projected. The first later sample remains retained in the source so the
    /// next decoder can consume its SPS/PPS/IDR boundary.
    EpochAdvanced { next_epoch: ShareEpoch },
}

/// One depacketized remote sample carrying its authenticated RTP media epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::peer_session) struct RemoteVideoSample {
    pub epoch: ShareEpoch,
    pub sample: EncodedVideoSample,
}

/// Read-only stream of encoded video received from one live session.
#[derive(Debug)]
pub(crate) struct RemoteVideoSource {
    rx: broadcast::Receiver<RemoteVideoSample>,
    pending: Option<RemoteVideoSample>,
    /// A broadcast overwrite occurred before the next retained sample. Keep
    /// this sticky across stale samples and epoch handoff until it can be
    /// attached to the first sample that a decoder may consume.
    discontinuous: bool,
}

impl RemoteVideoSource {
    pub(in crate::peer_session) fn new(rx: broadcast::Receiver<RemoteVideoSample>) -> Self {
        Self { rx, pending: None, discontinuous: false }
    }

    /// Returns the next encoded sample belonging to exactly `epoch`.
    ///
    /// Older samples are consumed and discarded. The first newer sample is
    /// retained and reported as an epoch advance so an old decoder cannot
    /// consume the new share's keyframe before its control event is projected.
    pub(crate) async fn recv_for(&mut self, epoch: ShareEpoch) -> Result<RemoteVideoRead, Error> {
        epoch.require_valid()?;
        loop {
            let tagged = match self.pending.take() {
                Some(pending) => pending,
                None => match self.rx.recv().await {
                    Ok(tagged) => tagged,
                    Err(broadcast::error::RecvError::Closed) => {
                        return Err(Error::MediaClosed);
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        self.discontinuous = true;
                        tracing::debug!(
                            skipped,
                            "remote video source lagged; marking retained media discontinuous"
                        );
                        continue;
                    }
                },
            };
            match tagged.epoch.cmp(&epoch) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => {
                    let mut sample = tagged.sample;
                    if std::mem::take(&mut self.discontinuous) {
                        sample.mark_discontinuous();
                    }
                    return Ok(RemoteVideoRead::Sample(sample));
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

pub(in crate::peer_session) fn remote_video_channel(
    capacity: usize,
) -> (broadcast::Sender<RemoteVideoSample>, broadcast::Receiver<RemoteVideoSample>) {
    broadcast::channel(capacity.max(1))
}

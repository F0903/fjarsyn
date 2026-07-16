use std::time::Duration;

use bytes::Bytes;
use tokio::sync::{broadcast, mpsc};

use super::{PeerSessionError, SessionId};

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

/// Bounded capability for submitting encoded video to one live session.
#[derive(Debug, Clone)]
pub struct EncodedVideoSink {
    session_id: SessionId,
    tx: mpsc::Sender<EncodedVideoSample>,
}

impl EncodedVideoSink {
    pub(crate) fn new(session_id: SessionId, tx: mpsc::Sender<EncodedVideoSample>) -> Self {
        Self { session_id, tx }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub async fn send(&self, sample: EncodedVideoSample) -> Result<(), PeerSessionError> {
        self.tx.send(sample).await.map_err(|_| PeerSessionError::MediaClosed)
    }

    pub fn try_send(&self, sample: EncodedVideoSample) -> Result<(), PeerSessionError> {
        self.tx.try_send(sample).map_err(|error| match error {
            mpsc::error::TrySendError::Closed(_) => PeerSessionError::MediaClosed,
            mpsc::error::TrySendError::Full(_) => {
                PeerSessionError::Protocol("encoded-video queue is full".into())
            }
        })
    }
}

/// Read-only stream of encoded video received from one live session.
#[derive(Debug)]
pub struct RemoteVideoSource {
    session_id: SessionId,
    rx: broadcast::Receiver<EncodedVideoSample>,
}

impl RemoteVideoSource {
    pub(crate) fn new(session_id: SessionId, rx: broadcast::Receiver<EncodedVideoSample>) -> Self {
        Self { session_id, rx }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub async fn recv(&mut self) -> Result<EncodedVideoSample, PeerSessionError> {
        self.rx.recv().await.map_err(|error| match error {
            broadcast::error::RecvError::Closed => PeerSessionError::MediaClosed,
            broadcast::error::RecvError::Lagged(skipped) => {
                PeerSessionError::RemoteVideoLagged { skipped }
            }
        })
    }
}

pub(crate) fn encoded_video_channel(
    session_id: SessionId,
    capacity: usize,
) -> (EncodedVideoSink, mpsc::Receiver<EncodedVideoSample>) {
    let (tx, rx) = mpsc::channel(capacity.max(1));
    (EncodedVideoSink::new(session_id, tx), rx)
}

pub(crate) fn remote_video_channel(
    capacity: usize,
) -> (broadcast::Sender<EncodedVideoSample>, broadcast::Receiver<EncodedVideoSample>) {
    broadcast::channel(capacity.max(1))
}

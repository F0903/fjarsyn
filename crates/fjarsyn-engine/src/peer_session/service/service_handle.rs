use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::orchestration::{Command, TrustBarrierOwnerId};
use crate::{
    identity::PeerId,
    peer_session::{
        EncodedVideoSink, Error, Event, MessageId, RemoteVideoSource, SessionId, ShareId, Snapshot,
        actor,
    },
};

#[derive(Debug, Clone)]
pub struct ServiceHandle {
    command_tx: mpsc::Sender<Command>,
    snapshot_rx: watch::Receiver<Snapshot>,
    event_tx: broadcast::Sender<Event>,
    operation_timeout: Duration,
}

impl ServiceHandle {
    pub(super) fn new(
        command_tx: mpsc::Sender<Command>,
        snapshot_rx: watch::Receiver<Snapshot>,
        event_tx: broadcast::Sender<Event>,
        operation_timeout: Duration,
    ) -> Self {
        Self { command_tx, snapshot_rx, event_tx, operation_timeout }
    }

    #[cfg(test)]
    pub(super) fn command_sender(&self) -> mpsc::Sender<Command> {
        self.command_tx.clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_rx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.snapshot_rx.clone()
    }

    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.event_tx.subscribe()
    }

    pub async fn connect(&self, peer_id: PeerId) -> Result<SessionId, Error> {
        self.send_command(|reply| Command::Connect { peer_id, reply }).await
    }

    /// Idempotently installs the application-owned trust-mutation barrier.
    /// The first installation prevents admission and closes an authenticated
    /// session before returning; retries reassert the same barrier.
    pub(crate) async fn ensure_trust_suspended(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), Error> {
        self.send_command(|reply| Command::EnsureTrustSuspended { peer_id, owner_id, reply }).await
    }

    /// Idempotently releases the application-owned trust-mutation barrier.
    pub(crate) async fn release_trust_suspension(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), Error> {
        self.send_command(|reply| Command::ReleaseTrustSuspension { peer_id, owner_id, reply })
            .await
    }

    pub async fn accept(&self, session_id: SessionId) -> Result<(), Error> {
        self.session_command(session_id, actor::Command::Accept).await
    }

    pub async fn reject(
        &self,
        session_id: SessionId,
        reason: impl Into<String>,
    ) -> Result<(), Error> {
        let reason = reason.into();
        self.session_command(session_id, |reply| actor::Command::Reject { reason, reply }).await
    }

    pub async fn disconnect(&self, session_id: SessionId) -> Result<(), Error> {
        self.session_command(session_id, actor::Command::Disconnect).await
    }

    pub async fn send_message(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        body: impl Into<String>,
        sent_at: DateTime<Utc>,
    ) -> Result<(), Error> {
        let body = body.into();
        self.session_command(session_id, |reply| actor::Command::SendMessage {
            message_id,
            body,
            sent_at,
            reply,
        })
        .await
    }

    pub async fn send_receipt(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), Error> {
        self.session_command(session_id, |reply| actor::Command::SendReceipt {
            message_id,
            received_at,
            reply,
        })
        .await
    }

    pub(crate) async fn start_screen_share(&self, session_id: SessionId) -> Result<ShareId, Error> {
        self.session_command(session_id, actor::Command::StartShare).await
    }

    pub(crate) async fn stop_screen_share(
        &self,
        session_id: SessionId,
        share_id: ShareId,
    ) -> Result<(), Error> {
        self.session_command(session_id, |reply| actor::Command::StopShare { share_id, reply })
            .await
    }

    #[cfg(test)]
    pub(super) async fn force_ice_restart(&self, session_id: SessionId) -> Result<(), Error> {
        self.session_command(session_id, actor::Command::ForceIceRestart).await
    }

    #[cfg(test)]
    pub(super) async fn committed_transport_generation(
        &self,
        session_id: SessionId,
    ) -> Result<u64, Error> {
        self.session_command(session_id, actor::Command::CommittedTransportGeneration)
            .await
            .map(crate::peer_session::TransportGeneration::value)
    }

    pub(crate) async fn encoded_video_sink(
        &self,
        session_id: SessionId,
        share_id: ShareId,
    ) -> Result<EncodedVideoSink, Error> {
        self.send_command(|reply| Command::EncodedVideoSink { session_id, share_id, reply }).await
    }

    pub(crate) async fn subscribe_remote_video(
        &self,
        session_id: SessionId,
    ) -> Result<RemoteVideoSource, Error> {
        self.send_command(|reply| Command::RemoteVideoSource { session_id, reply }).await
    }

    async fn session_command<T>(
        &self,
        session_id: SessionId,
        build: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> actor::Command,
    ) -> Result<T, Error> {
        self.send_command(|reply| Command::Session { session_id, command: build(reply) }).await
    }

    async fn send_command<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> Command,
    ) -> Result<T, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        tokio::time::timeout(self.operation_timeout, self.command_tx.send(build(reply_tx)))
            .await
            .map_err(|_| Error::OperationTimeout)?
            .map_err(|_| Error::ServiceStopped)?;
        // Once accepted by the service queue, mutations have definitive completion
        // semantics. Every underlying network/RTC operation is independently bounded,
        // so returning a caller timeout here would allow a queued mutation to run later.
        reply_rx.await.map_err(|_| Error::ResponseDropped)?
    }
}

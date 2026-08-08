//! Actor-owned ICE-restart lifecycle, connection routing, and cleanup task ownership.

use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
    time::Instant,
};

use crate::{
    identity::PeerId,
    peer_session::{
        Error, Phase, SessionId, TransportGeneration,
        actor::Role,
        negotiation,
        protocol::NegotiationSignal,
        restart::{Attempt, Coordinator},
    },
};

const MAX_REJECTION_TASKS: usize = 4;

#[derive(Debug)]
pub(in crate::peer_session) struct Attachment {
    pub(in crate::peer_session) generation: TransportGeneration,
    pub(in crate::peer_session) connection: negotiation::Connection,
}

pub(in crate::peer_session::actor) struct DialResult {
    pub(in crate::peer_session::actor) generation: TransportGeneration,
    pub(in crate::peer_session::actor) result: Result<negotiation::Connection, Error>,
}

pub(in crate::peer_session::actor) enum Event {
    DeadlineElapsed,
    RejectionTaskFailed,
    Attachment(Attachment),
    DialCompleted(DialResult),
    DialTaskFailed,
}

pub(in crate::peer_session::actor) struct Controller {
    coordinator: Coordinator,
    sdp_applied: bool,
    old_transport_recovery_allowed: bool,
    attachment_rx: mpsc::Receiver<Attachment>,
    attachment_channel_open: bool,
    dial_task: Option<JoinHandle<DialResult>>,
    connection_origin: Option<ConnectionOrigin>,
    rejection_tasks: JoinSet<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionOrigin {
    Dialed,
    Attached,
}

impl Controller {
    pub(in crate::peer_session::actor) fn new() -> (Self, mpsc::Sender<Attachment>) {
        let (attachment_tx, attachment_rx) = mpsc::channel(1);
        (
            Self {
                coordinator: Coordinator::default(),
                sdp_applied: false,
                old_transport_recovery_allowed: true,
                attachment_rx,
                attachment_channel_open: true,
                dial_task: None,
                connection_origin: None,
                rejection_tasks: JoinSet::new(),
            },
            attachment_tx,
        )
    }

    pub(in crate::peer_session::actor) fn committed(&self) -> TransportGeneration {
        self.coordinator.committed()
    }

    pub(in crate::peer_session::actor) fn active(&self) -> Option<Attempt> {
        self.coordinator.active()
    }

    pub(in crate::peer_session::actor) fn require_active(
        &self,
        generation: TransportGeneration,
    ) -> Result<Attempt, Error> {
        self.coordinator.require_active(generation)
    }

    pub(in crate::peer_session::actor) fn begin_local(
        &mut self,
        deadline: Instant,
        old_transport_recovery_allowed: bool,
    ) -> Result<TransportGeneration, Error> {
        let generation = self.coordinator.begin_local(deadline)?;
        self.old_transport_recovery_allowed = old_transport_recovery_allowed;
        self.sdp_applied = false;
        Ok(generation)
    }

    pub(in crate::peer_session::actor) fn begin_remote(
        &mut self,
        generation: TransportGeneration,
        deadline: Instant,
    ) -> Result<(), Error> {
        self.coordinator.begin_remote(generation, deadline)?;
        self.old_transport_recovery_allowed = true;
        self.sdp_applied = false;
        Ok(())
    }

    pub(in crate::peer_session::actor) fn engage(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        self.coordinator.engage(generation)
    }

    pub(in crate::peer_session::actor) fn authorize(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        self.coordinator.authorize(generation)
    }

    pub(in crate::peer_session::actor) fn can_cancel(&self) -> bool {
        self.coordinator.can_cancel()
    }

    pub(in crate::peer_session::actor) fn cancel(&mut self) -> Result<(), Error> {
        self.coordinator.cancel()?;
        self.finish_attempt();
        Ok(())
    }

    pub(in crate::peer_session::actor) fn commit(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        self.coordinator.commit(generation)?;
        self.finish_attempt();
        Ok(())
    }

    fn finish_attempt(&mut self) {
        self.old_transport_recovery_allowed = true;
        self.connection_origin = None;
    }

    pub(in crate::peer_session::actor) fn old_transport_recovery_allowed(&self) -> bool {
        self.old_transport_recovery_allowed
    }

    pub(in crate::peer_session::actor) fn mark_sdp_applied(&mut self) {
        self.sdp_applied = true;
    }

    pub(in crate::peer_session::actor) fn sdp_applied(&self) -> bool {
        self.sdp_applied
    }

    pub(in crate::peer_session::actor) fn spawn_dial(
        &mut self,
        negotiation: negotiation::Service,
        session_id: SessionId,
        peer_id: PeerId,
        generation: TransportGeneration,
        deadline: Instant,
    ) {
        debug_assert!(self.dial_task.is_none());
        self.dial_task = Some(tokio::spawn(async move {
            let result =
                tokio::time::timeout_at(deadline, negotiation.connect(session_id, peer_id))
                    .await
                    .map_err(|_| Error::Signaling("ICE restart signaling timed out".into()))
                    .and_then(|result| result);
            DialResult { generation, result }
        }));
    }

    pub(in crate::peer_session::actor) fn dial_running(&self) -> bool {
        self.dial_task.is_some()
    }

    pub(in crate::peer_session::actor) async fn next_event(&mut self, phase: Phase) -> Event {
        loop {
            let deadline = if phase == Phase::Reconnecting {
                self.active().map(|attempt| attempt.deadline())
            } else {
                None
            };
            let has_rejection_tasks = !self.rejection_tasks.is_empty();
            let attachment_channel_open = self.attachment_channel_open;
            let dial_running = self.dial_task.is_some();

            tokio::select! {
                biased;
                _ = wait_for_deadline(deadline) => return Event::DeadlineElapsed,
                joined = self.rejection_tasks.join_next(), if has_rejection_tasks => {
                    if joined.is_some_and(|result| result.is_err()) {
                        return Event::RejectionTaskFailed;
                    }
                }
                attachment = self.attachment_rx.recv(), if attachment_channel_open => {
                    match attachment {
                        Some(attachment) => return Event::Attachment(attachment),
                        None => self.attachment_channel_open = false,
                    }
                }
                joined = async {
                    self.dial_task.as_mut().expect("dial task checked above").await
                }, if dial_running => {
                    self.dial_task.take();
                    return match joined {
                        Ok(result) => Event::DialCompleted(result),
                        Err(_) => Event::DialTaskFailed,
                    };
                }
            }
        }
    }

    pub(in crate::peer_session::actor) fn conflicts_with_canonical_connection(
        &self,
        role: Role,
    ) -> bool {
        self.connection_origin == Some(ConnectionOrigin::Attached)
            || (role == Role::Outgoing
                && (self.dial_running()
                    || self.connection_origin == Some(ConnectionOrigin::Dialed)))
            || (role == Role::Incoming
                && self.connection_origin == Some(ConnectionOrigin::Dialed)
                && self.active().is_some_and(|attempt| attempt.authorized()))
    }

    pub(in crate::peer_session::actor) fn mark_connection_dialed(&mut self) {
        self.connection_origin = Some(ConnectionOrigin::Dialed);
    }

    pub(in crate::peer_session::actor) fn mark_connection_attached(&mut self) {
        self.connection_origin = Some(ConnectionOrigin::Attached);
    }

    pub(in crate::peer_session::actor) fn connection_was_dialed(&self) -> bool {
        self.connection_origin == Some(ConnectionOrigin::Dialed)
    }

    pub(in crate::peer_session::actor) fn clear_connection_origin(&mut self) {
        self.connection_origin = None;
    }

    pub(in crate::peer_session::actor) fn reject_connection(
        &mut self,
        connection: negotiation::Connection,
        reason: impl Into<String>,
        cleanup_timeout: std::time::Duration,
    ) {
        if self.rejection_tasks.len() >= MAX_REJECTION_TASKS {
            return;
        }
        let deadline = self
            .active()
            .map(|attempt| attempt.deadline())
            .unwrap_or_else(|| Instant::now() + cleanup_timeout)
            .min(Instant::now() + cleanup_timeout);
        let reason = reason.into();
        self.rejection_tasks.spawn(async move {
            let _ = tokio::time::timeout_at(
                deadline,
                connection.send(NegotiationSignal::Reject { reason }),
            )
            .await;
            connection.shutdown_until(deadline).await;
        });
    }

    pub(in crate::peer_session::actor) async fn abort_dial(&mut self, deadline: Instant) {
        if let Some(task) = self.dial_task.take() {
            task.abort();
            if Instant::now() >= deadline {
                return;
            }
            if let Ok(Ok(DialResult { result: Ok(connection), .. })) =
                tokio::time::timeout_at(deadline, task).await
                && Instant::now() < deadline
            {
                connection.shutdown_until(deadline).await;
            }
        }
    }

    pub(in crate::peer_session::actor) async fn shutdown(&mut self, deadline: Instant) {
        self.abort_dial(deadline).await;
        self.rejection_tasks.abort_all();
        while !self.rejection_tasks.is_empty() && Instant::now() < deadline {
            if tokio::time::timeout_at(deadline, self.rejection_tasks.join_next()).await.is_err() {
                break;
            }
        }
        while let Ok(attachment) = self.attachment_rx.try_recv() {
            if Instant::now() < deadline {
                attachment.connection.shutdown_until(deadline).await;
            }
        }
        self.connection_origin = None;
    }

    pub(in crate::peer_session::actor) fn require_transport_generation(
        &self,
        phase: Phase,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        let expected = match phase {
            Phase::Negotiating => self.committed(),
            Phase::Reconnecting => self
                .active()
                .ok_or_else(|| Error::Protocol("missing ICE restart attempt".into()))?
                .generation(),
            _ => {
                return Err(Error::Protocol(
                    "transport signaling arrived outside negotiation".into(),
                ));
            }
        };
        if generation != expected {
            return Err(Error::Protocol("signaling used the wrong transport generation".into()));
        }
        Ok(())
    }

    pub(in crate::peer_session::actor) fn active_transport_generation(
        &self,
        phase: Phase,
    ) -> Result<TransportGeneration, Error> {
        match phase {
            Phase::Negotiating => Ok(self.committed()),
            Phase::Reconnecting => self
                .active()
                .map(|attempt| attempt.generation())
                .ok_or_else(|| Error::Protocol("missing ICE restart attempt".into())),
            _ => Err(Error::Protocol("session has no active transport negotiation".into())),
        }
    }

    pub(in crate::peer_session::actor) fn generation_is_current(
        &self,
        phase: Phase,
        generation: TransportGeneration,
    ) -> bool {
        match phase {
            Phase::Reconnecting => {
                self.active().is_some_and(|attempt| attempt.generation() == generation)
            }
            _ => self.committed() == generation,
        }
    }

    pub(in crate::peer_session::actor) fn event_is_relevant(
        &self,
        generation: TransportGeneration,
    ) -> bool {
        self.committed() == generation
            || self.active().is_some_and(|attempt| attempt.generation() == generation)
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if let Some(task) = self.dial_task.take() {
            task.abort();
        }
    }
}

async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

use std::time::Instant;

use tokio::{sync::mpsc, time::Instant as TokioInstant};

use super::{
    super::Command, Runtime, sessions::drain_pending_session_updates,
    shutdown::receive_shutdown_deadline,
};
use crate::{
    identity::PeerId,
    peer_session::{
        CloseReason, Error, Event, Phase, SessionId,
        actor::{Role, Update},
    },
};

impl Runtime {
    pub(super) async fn handle_command(&mut self, command: Command) {
        match command {
            Command::Connect { peer_id, reply } => {
                let mut shutdown_rx = self.shutdown_rx.clone();
                let result = tokio::select! {
                    biased;
                    _ = receive_shutdown_deadline(&mut shutdown_rx) => {
                        Err(Error::ServiceStopped)
                    }
                    result = self.connect(peer_id) => result,
                };
                let _ = reply.send(result);
            }
            Command::EnsureTrustSuspended { peer_id, owner_id, reply } => {
                let first_owner = {
                    let owners = self.suspended_peers.entry(peer_id.clone()).or_default();
                    let first_owner = owners.is_empty();
                    owners.insert(owner_id);
                    first_owner
                };
                if first_owner {
                    self.terminate_suspended_peer(&peer_id).await;
                }
                let _ = reply.send(Ok(()));
            }
            Command::ReleaseTrustSuspension { peer_id, owner_id, reply } => {
                let remove_peer = self.suspended_peers.get_mut(&peer_id).is_some_and(|owners| {
                    owners.remove(&owner_id);
                    owners.is_empty()
                });
                if remove_peer {
                    self.suspended_peers.remove(&peer_id);
                }
                let _ = reply.send(Ok(()));
            }
            Command::Session { session_id, command } => {
                let Some(entry) = self.sessions.get(&session_id) else {
                    command.reply_error(Error::SessionNotFound(session_id));
                    return;
                };
                if let Err(error) = entry.handle.command_tx().try_send(command) {
                    match error {
                        mpsc::error::TrySendError::Full(command) => {
                            command.reply_error(Error::SessionBusy(session_id));
                        }
                        mpsc::error::TrySendError::Closed(command) => {
                            command.reply_error(Error::ServiceStopped);
                        }
                    }
                }
            }
            Command::EncodedVideoSink { session_id, share_id, reply } => {
                let result = self
                    .connected_entry(session_id)
                    .and_then(|entry| entry.handle.encoded_video_sink(share_id));
                let _ = reply.send(result);
            }
            Command::RemoteVideoSource { session_id, reply } => {
                let result = self
                    .connected_entry(session_id)
                    .map(|entry| entry.handle.remote_video_source());
                let _ = reply.send(result);
            }
        }
    }

    async fn connect(&mut self, peer_id: PeerId) -> Result<SessionId, Error> {
        tokio::time::timeout(self.limits.service_operation_timeout, self.connect_inner(peer_id))
            .await
            .map_err(|_| Error::OperationTimeout)?
    }

    async fn connect_inner(&mut self, peer_id: PeerId) -> Result<SessionId, Error> {
        if self.mandatory_event_sink_failed {
            return Err(Error::Protocol(
                "reliable peer-session event delivery is unavailable".into(),
            ));
        }
        if peer_id == self.local_peer_id {
            return Err(Error::Protocol("cannot connect to the local peer".into()));
        }
        if self.suspended_peers.contains_key(&peer_id) {
            return Err(Error::PeerSuspended(peer_id));
        }
        if self.peers.contains_key(&peer_id) {
            return Err(Error::SessionAlreadyExists(peer_id));
        }
        if self.sessions.len() >= self.limits.max_sessions {
            return Err(Error::Protocol("session capacity reached".into()));
        }
        let session_id = loop {
            let candidate = SessionId::new();
            if !self.recent_session_ids.seen_or_remember(candidate, Instant::now()) {
                break candidate;
            }
        };
        let connection = self.negotiation.connect(session_id, peer_id.clone()).await?;
        self.insert_session(session_id, peer_id, Role::Outgoing, connection).await?;
        Ok(session_id)
    }

    async fn terminate_suspended_peer(&mut self, peer_id: &PeerId) {
        let Some(session_id) = self.peers.remove(peer_id) else {
            return;
        };
        let Some(mut entry) = self.sessions.remove(&session_id) else {
            return;
        };
        let generation = entry.handle.generation;
        let deadline = TokioInstant::now() + self.limits.shutdown_timeout;
        entry.handle.revoke_trust(deadline);
        if tokio::time::timeout_at(deadline, &mut entry.task).await.is_err() {
            tracing::warn!(%session_id, %peer_id, "aborting peer session while suspending trust");
            entry.task.abort();
            // The revocation budget has expired. Detach cooperative actor
            // unwinding instead of extending the command beyond its fence.
        }

        // The actor publishes all semantic updates before its terminal marker.
        // Preserve that ordering even though this service command owns removal.
        for update in drain_pending_session_updates(&mut self.update_rx) {
            match update {
                Update { generation: update_generation, event }
                    if update_generation == generation && event.session_id() == session_id =>
                {
                    self.emit(event).await;
                }
                update => self.handle_update(update).await,
            }
        }

        let mut close_reason = CloseReason::TrustRevoked;
        let mut other_terminals = Vec::new();
        while let Ok(terminal) = self.terminal_rx.try_recv() {
            if terminal.generation == generation && terminal.session_id == session_id {
                close_reason = terminal.reason;
            } else {
                other_terminals.push(terminal);
            }
        }
        for terminal in other_terminals {
            self.handle_terminal(terminal).await;
        }

        self.publish_snapshot();
        self.emit(Event::Closed { session_id, peer_id: peer_id.clone(), reason: close_reason })
            .await;
    }

    fn connected_entry(&self, session_id: SessionId) -> Result<&super::SessionEntry, Error> {
        let entry = self.sessions.get(&session_id).ok_or(Error::SessionNotFound(session_id))?;
        let snapshot = entry.handle.snapshot();
        if snapshot.phase != Phase::Connected {
            return Err(Error::InvalidState {
                session_id,
                phase: snapshot.phase.name(),
                operation: "access session media",
            });
        }
        Ok(entry)
    }
}

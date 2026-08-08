use std::time::{Duration, Instant};

use tokio::{sync::oneshot, time::Instant as TokioInstant};

use super::Runtime;
use crate::{
    identity::PeerId,
    peer_session::{
        Event, Phase, TransportGeneration,
        actor::{self, Role, restart::Attachment},
        negotiation,
        protocol::NegotiationSignal,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncomingRequestResolution {
    Prompt,
    KeepOutgoing,
    ReplaceAndAccept,
    RejectExistingSession,
}

fn resolve_incoming_request(
    local_peer_id: &PeerId,
    remote_peer_id: &PeerId,
    existing_phase: Option<Phase>,
) -> IncomingRequestResolution {
    match existing_phase {
        None => IncomingRequestResolution::Prompt,
        Some(Phase::Requesting) if local_peer_id < remote_peer_id => {
            IncomingRequestResolution::KeepOutgoing
        }
        Some(Phase::Requesting) => IncomingRequestResolution::ReplaceAndAccept,
        Some(_) => IncomingRequestResolution::RejectExistingSession,
    }
}

impl Runtime {
    pub(super) async fn handle_incoming(&mut self, incoming: negotiation::Incoming) {
        let reject_timeout =
            self.limits.signaling_handshake_timeout.min(self.limits.service_operation_timeout);
        if self.mandatory_event_sink_failed {
            reject_connection(
                incoming.connection,
                "reliable peer-session event delivery is unavailable",
                reject_timeout,
            )
            .await;
            return;
        }
        if incoming.peer_id == self.local_peer_id {
            reject_connection(
                incoming.connection,
                "cannot connect to the local peer",
                reject_timeout,
            )
            .await;
            return;
        }
        if self.suspended_peers.contains_key(&incoming.peer_id) {
            reject_connection(incoming.connection, "peer identity is suspended", reject_timeout)
                .await;
            return;
        }
        let current_trusted_peer = tokio::time::timeout(
            self.limits.service_operation_timeout,
            self.trusted_peers.trusted_peer(&incoming.peer_id),
        )
        .await;
        let current_trusted_peer = match current_trusted_peer {
            Ok(Ok(Some(trusted_peer))) => trusted_peer,
            _ => {
                reject_connection(
                    incoming.connection,
                    "peer identity is no longer trusted",
                    reject_timeout,
                )
                .await;
                return;
            }
        };
        if current_trusted_peer.public_key != incoming.authenticated_public_key {
            reject_connection(
                incoming.connection,
                "peer identity changed during authentication",
                reject_timeout,
            )
            .await;
            return;
        }
        if let negotiation::Intent::Restart { generation } = incoming.intent {
            self.handle_incoming_restart(incoming, generation);
            return;
        }
        if self.recent_session_ids.seen_or_remember(incoming.session_id, Instant::now()) {
            reject_connection(
                incoming.connection,
                "session identifier was already used",
                reject_timeout,
            )
            .await;
            return;
        }

        let mut auto_accept = false;
        if let Some(existing_id) = self.peers.get(&incoming.peer_id).copied() {
            let existing_phase =
                self.sessions.get(&existing_id).map(|entry| entry.handle.snapshot().phase);
            match resolve_incoming_request(&self.local_peer_id, &incoming.peer_id, existing_phase) {
                IncomingRequestResolution::RejectExistingSession => {
                    reject_connection(
                        incoming.connection,
                        "a session with this peer already exists",
                        reject_timeout,
                    )
                    .await;
                    return;
                }
                IncomingRequestResolution::KeepOutgoing => {
                    reject_connection(
                        incoming.connection,
                        "simultaneous request superseded",
                        reject_timeout,
                    )
                    .await;
                    return;
                }
                IncomingRequestResolution::ReplaceAndAccept => {}
                IncomingRequestResolution::Prompt => unreachable!("existing session was present"),
            }

            if let Some(mut existing) = self.sessions.remove(&existing_id) {
                self.peers.remove(&incoming.peer_id);
                existing.handle.fail("simultaneous outgoing session was superseded");
                if tokio::time::timeout(self.limits.shutdown_timeout, &mut existing.task)
                    .await
                    .is_err()
                {
                    existing.task.abort();
                    // The replacement budget is exhausted. Dropping the
                    // aborted handle detaches cooperative actor unwinding;
                    // generations keep any late updates from affecting the
                    // replacement session.
                }
            } else {
                self.peers.remove(&incoming.peer_id);
            }

            auto_accept = true;
        } else if self.sessions.len() >= self.limits.max_sessions {
            reject_connection(incoming.connection, "session capacity reached", reject_timeout)
                .await;
            return;
        }

        let session_id = incoming.session_id;
        let peer_id = incoming.peer_id;
        if self.sessions.contains_key(&session_id) {
            reject_connection(incoming.connection, "session identifier collision", reject_timeout)
                .await;
            return;
        }
        if self
            .insert_session(session_id, peer_id.clone(), Role::Incoming, incoming.connection)
            .await
            .is_ok()
        {
            if auto_accept {
                if let Some(entry) = self.sessions.get(&session_id) {
                    let (reply, _ignored) = oneshot::channel();
                    if entry.handle.command_tx().try_send(actor::Command::Accept(reply)).is_err() {
                        entry.handle.fail("automatic simultaneous-connect acceptance failed");
                    }
                }
            } else {
                self.emit(Event::IncomingRequest { session_id, peer_id }).await;
            }
        }
    }

    fn handle_incoming_restart(
        &mut self,
        incoming: negotiation::Incoming,
        generation: TransportGeneration,
    ) {
        let Some(entry) = self.sessions.get(&incoming.session_id) else {
            discard_restart_connection(
                incoming.connection,
                "restart does not identify an active session",
            );
            return;
        };
        if entry.handle.snapshot().peer_id != incoming.peer_id
            || self.peers.get(&incoming.peer_id) != Some(&incoming.session_id)
        {
            discard_restart_connection(
                incoming.connection,
                "restart peer and session do not match",
            );
            return;
        }
        if !matches!(entry.handle.snapshot().phase, Phase::Connected | Phase::Reconnecting) {
            discard_restart_connection(
                incoming.connection,
                "session is not eligible for ICE restart",
            );
            return;
        }

        let attachment = Attachment { generation, connection: incoming.connection };
        if let Err(error) = entry.handle.try_attach_restart(attachment) {
            let attachment = *error;
            discard_restart_connection(
                attachment.connection,
                "session cannot accept restart signaling",
            );
        }
    }
}

fn discard_restart_connection(connection: negotiation::Connection, reason: &str) {
    tracing::debug!(?connection, reason, "discarding invalid ICE restart signaling");
    drop(connection);
}

async fn reject_connection(connection: negotiation::Connection, reason: &str, timeout: Duration) {
    let deadline = TokioInstant::now() + timeout;
    let _ = tokio::time::timeout_at(
        deadline,
        connection.send(NegotiationSignal::Reject { reason: reason.to_owned() }),
    )
    .await;
    connection.shutdown_until(deadline).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_connect_converges_on_lower_peer_id_as_offerer() {
        let lower = PeerId::new("a").unwrap();
        let higher = PeerId::new("b").unwrap();

        assert_eq!(
            resolve_incoming_request(&lower, &higher, Some(Phase::Requesting)),
            IncomingRequestResolution::KeepOutgoing
        );
        assert_eq!(
            resolve_incoming_request(&higher, &lower, Some(Phase::Requesting)),
            IncomingRequestResolution::ReplaceAndAccept
        );
    }

    #[test]
    fn incoming_request_never_evicts_a_non_requesting_session() {
        let local = PeerId::new("a").unwrap();
        let remote = PeerId::new("b").unwrap();
        for phase in [Phase::Incoming, Phase::Negotiating, Phase::Connected, Phase::Disconnecting] {
            assert_eq!(
                resolve_incoming_request(&local, &remote, Some(phase)),
                IncomingRequestResolution::RejectExistingSession,
                "phase={phase:?}"
            );
        }
    }
}

use std::sync::Arc;

use tokio::sync::mpsc;

use super::{Runtime, SessionEntry};
use crate::{
    identity::PeerId,
    peer_session::{
        CloseReason, Error, Event, SessionId, Sessions,
        actor::{self, Role, TaskExit, Terminal, Update},
        negotiation, rtc,
    },
};

impl Runtime {
    pub(super) async fn insert_session(
        &mut self,
        session_id: SessionId,
        peer_id: PeerId,
        role: Role,
        connection: negotiation::Connection,
    ) -> Result<(), Error> {
        let remote_public_key = connection.authenticated_remote_public_key().to_owned();
        let rtc = rtc::Config {
            network_scope: self.network_scope,
            ice_servers: self.ice_servers.clone(),
            max_depacket_latency: self.max_depacket_latency,
            max_candidates_per_generation: self.limits.max_ice_candidates_per_generation,
            max_data_message_bytes: self.limits.max_data_message_bytes,
            operation_timeout: self.limits.rtc_operation_timeout,
        };
        let config = actor::Config {
            session_id,
            remote_peer_id: peer_id.clone(),
            remote_public_key,
            role,
            connection: Some(connection),
            negotiation: self.negotiation.clone(),
            rtc,
            command_capacity: self.limits.session_command_capacity,
            media_capacity: self.limits.video_input_capacity,
            remote_video_capacity: self.limits.remote_video_capacity,
            max_message_bytes: self.limits.max_message_bytes,
            max_data_message_bytes: self.limits.max_data_message_bytes,
            request_timeout: self.limits.request_timeout,
            negotiation_timeout: self.limits.negotiation_timeout,
            event_delivery_timeout: self.limits.event_delivery_timeout,
            cleanup_timeout: self.limits.shutdown_timeout,
            pre_ready_data_capacity: self.limits.pre_ready_data_capacity.max(1),
            disconnected_grace: self.limits.disconnected_grace,
            ice_restart_timeout: self.limits.ice_restart_timeout,
            max_remote_timestamp_age: self.limits.max_remote_timestamp_age,
            max_remote_clock_skew: self.limits.max_remote_clock_skew,
        };
        let (handle, task) = actor::spawn(
            config,
            self.update_tx.clone(),
            self.terminal_tx.clone(),
            self.task_exit_tx.clone(),
        );
        self.peers.insert(peer_id, session_id);
        self.sessions.insert(session_id, SessionEntry::new(handle, task));
        self.publish_snapshot();
        Ok(())
    }

    pub(super) async fn handle_update(&mut self, update: Update) {
        let Update { instance_id, event } = update;
        if self
            .sessions
            .get(&event.session_id())
            .is_some_and(|entry| entry.handle.instance_id == instance_id)
        {
            self.emit(event).await;
        }
    }

    pub(super) async fn handle_terminal(&mut self, terminal: Terminal) {
        // The actor sends its terminal marker only after every semantic update
        // send has completed. The separate terminal channel can nevertheless win
        // select first, so drain the ordered update queue before removing the actor.
        for update in drain_pending_session_updates(&mut self.update_rx) {
            self.handle_update(update).await;
        }
        let Terminal { instance_id, session_id, peer_id, reason } = terminal;
        let current_instance_id =
            self.sessions.get(&session_id).map(|entry| entry.handle.instance_id);
        if current_instance_id != Some(instance_id) {
            return;
        }
        if let Some(mut entry) = self.sessions.remove(&session_id) {
            self.peers.remove(&peer_id);
            let _ = (&mut entry.task).await;
            self.publish_snapshot();

            self.emit(Event::Closed {
                session_id,
                peer_id: peer_id.clone(),
                reason: reason.clone(),
            })
            .await;
        }
        tracing::debug!(%session_id, %peer_id, ?reason, "peer session removed");
    }

    pub(super) async fn handle_task_exit(&mut self, task_exit: TaskExit) {
        let (instance_id, session_id, peer_id, panic_reason) = task_exit.into_parts();
        let current_instance_id =
            self.sessions.get(&session_id).map(|entry| entry.handle.instance_id);
        if current_instance_id != Some(instance_id) {
            return;
        }

        // A normal actor sends Terminal before its supervised future returns,
        // and the select loop prioritizes that semantic channel. Reaching this
        // branch while the actor instance is still registered therefore means the
        // actor escaped its normal cleanup path (usually through a panic).
        for update in drain_pending_session_updates(&mut self.update_rx) {
            self.handle_update(update).await;
        }
        let Some(mut entry) = self.sessions.remove(&session_id) else { return };
        self.peers.remove(&peer_id);
        let _ = (&mut entry.task).await;

        let detail = panic_reason
            .map(|reason| format!("session actor panicked: {reason}"))
            .unwrap_or_else(|| "session actor exited without a terminal signal".to_owned());
        tracing::error!(%session_id, %peer_id, reason = %detail, "peer session actor failed");
        self.publish_snapshot();
        self.emit(Event::Closed {
            session_id,
            peer_id,
            reason: CloseReason::ConnectionFailed { reason: detail },
        })
        .await;
    }

    pub(super) async fn emit(&mut self, event: Event) {
        let _ = self.event_tx.send(event.clone());
        let delivery_failed = self
            .mandatory_event_sink
            .as_ref()
            .is_some_and(|sink| sink.try_send(event.clone()).is_err());
        if delivery_failed {
            self.fail_mandatory_event_sink();
        }
    }

    pub(super) fn fail_mandatory_event_sink(&mut self) {
        if self.mandatory_event_sink_failed {
            return;
        }
        self.mandatory_event_sink.take();
        self.mandatory_event_sink_failed = true;
        tracing::error!(
            "reliable peer-session event sink overflowed or closed; terminating sessions"
        );
        for entry in self.sessions.values() {
            entry.handle.fail("mandatory peer-session event delivery failed");
        }
    }

    pub(super) fn publish_snapshot(&self) {
        let mut sessions =
            self.sessions.values().map(|entry| entry.handle.snapshot()).collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.session_id);
        let next = Sessions { sessions: Arc::new(sessions) };
        if *self.snapshot_tx.borrow() != next {
            self.snapshot_tx.send_replace(next);
        }
    }
}

pub(super) fn drain_pending_session_updates(update_rx: &mut mpsc::Receiver<Update>) -> Vec<Update> {
    let mut updates = Vec::new();
    while let Ok(update) = update_rx.try_recv() {
        updates.push(update);
    }
    updates
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::peer_session::{MessageId, actor::ActorInstanceId};

    #[tokio::test]
    async fn terminal_overtake_drains_semantic_updates_before_closed() {
        let session_id = SessionId::new();
        let peer_id = PeerId::new("peer").unwrap();
        let instance_id = ActorInstanceId::new();
        let message = Event::MessageReceived {
            session_id,
            peer_id: peer_id.clone(),
            message_id: MessageId::new(),
            body: "accepted before terminal".into(),
            sent_at: Utc::now(),
        };
        let (update_tx, mut update_rx) = mpsc::channel(2);
        update_tx.send(Update { instance_id, event: message.clone() }).await.unwrap();
        // Model the unbounded terminal channel winning select before update_rx.
        let terminal = Event::Closed { session_id, peer_id, reason: CloseReason::RemoteDisconnect };
        let (sink_tx, mut sink_rx) = mpsc::channel(2);
        for Update { event, .. } in drain_pending_session_updates(&mut update_rx) {
            sink_tx.try_send(event).unwrap();
        }
        sink_tx.try_send(terminal.clone()).unwrap();

        assert_eq!(sink_rx.recv().await.unwrap(), message);
        assert_eq!(sink_rx.recv().await.unwrap(), terminal);
    }
}

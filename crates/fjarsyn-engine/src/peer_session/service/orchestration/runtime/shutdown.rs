use std::{collections::HashMap, time::Duration};

use tokio::{sync::watch, time::Instant as TokioInstant};

use super::{super::Command, Runtime};
use crate::peer_session::{Error, Event, actor::Update};

impl Runtime {
    pub(super) async fn complete_shutdown(&mut self, deadline: TokioInstant) {
        self.command_rx.close();
        while let Ok(command) = self.command_rx.try_recv() {
            reply_service_error(command, Error::ServiceStopped);
        }
        let result = self.shutdown_sessions(deadline).await;
        self.mandatory_event_sink.take();
        if let Some(reply) = self.shutdown_complete_tx.take() {
            let _ = reply.send(result);
        }
    }

    pub(super) async fn complete_failure(&mut self, failure: Error, deadline: TokioInstant) {
        self.command_rx.close();
        while let Ok(command) = self.command_rx.try_recv() {
            reply_service_error(command, failure.clone());
        }
        let reason = failure.to_string();
        for entry in self.sessions.values() {
            entry.handle.fail(reason.clone());
        }
        tokio::task::yield_now().await;
        let shutdown_result = self.shutdown_sessions(deadline).await;
        self.mandatory_event_sink.take();
        let result = match shutdown_result {
            Ok(()) => Err(failure),
            Err(error) => Err(error),
        };
        if let Some(reply) = self.shutdown_complete_tx.take() {
            let _ = reply.send(result);
        }
    }

    async fn shutdown_sessions(&mut self, deadline: TokioInstant) -> Result<(), Error> {
        let active_generations = self
            .sessions
            .iter()
            .map(|(session_id, entry)| (*session_id, entry.handle.generation))
            .collect::<HashMap<_, _>>();
        let sessions = std::mem::take(&mut self.sessions);
        self.peers.clear();
        let actor_deadline = child_shutdown_deadline(deadline, self.limits.shutdown_timeout);
        for entry in sessions.values() {
            entry.handle.shutdown(actor_deadline);
        }
        let mut timed_out = false;
        for (_, mut entry) in sessions {
            if tokio::time::timeout_at(actor_deadline, &mut entry.task).await.is_err() {
                entry.task.abort();
                // The child deadline has expired. Dropping the entry detaches
                // cooperative actor cleanup without extending the owner budget.
                timed_out = true;
            }
        }
        while let Ok(Update { generation, event }) = self.update_rx.try_recv() {
            if active_generations.get(&event.session_id()) == Some(&generation) {
                self.emit(event).await;
            }
        }
        while let Ok(terminal) = self.terminal_rx.try_recv() {
            if active_generations.get(&terminal.session_id) == Some(&terminal.generation) {
                self.emit(Event::Closed {
                    session_id: terminal.session_id,
                    peer_id: terminal.peer_id,
                    reason: terminal.reason,
                })
                .await;
            }
        }
        self.publish_snapshot();
        if timed_out { Err(Error::ShutdownTimeout) } else { Ok(()) }
    }
}

fn reply_service_error(command: Command, error: Error) {
    match command {
        Command::Connect { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        Command::EnsureTrustSuspended { reply, .. }
        | Command::ReleaseTrustSuspension { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        Command::Session { command, .. } => command.reply_error(error),
        Command::EncodedVideoSink { reply, .. } => {
            let _ = reply.send(Err(error));
        }
        Command::RemoteVideoSource { reply, .. } => {
            let _ = reply.send(Err(error));
        }
    }
}

pub(super) async fn receive_shutdown_deadline(
    shutdown_rx: &mut watch::Receiver<Option<TokioInstant>>,
) -> TokioInstant {
    loop {
        if let Some(deadline) = *shutdown_rx.borrow() {
            return deadline;
        }
        if shutdown_rx.changed().await.is_err() {
            return TokioInstant::now();
        }
    }
}

fn child_shutdown_deadline(
    owner_deadline: TokioInstant,
    shutdown_timeout: Duration,
) -> TokioInstant {
    let cleanup_grace = shutdown_timeout
        .checked_div(10)
        .unwrap_or_default()
        .clamp(Duration::from_millis(100), Duration::from_millis(500));
    owner_deadline.checked_sub(cleanup_grace).unwrap_or(owner_deadline)
}

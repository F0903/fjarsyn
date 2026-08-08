use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};

use super::{
    CodecDirection, Command, Config, Output, Reconciler, StartOperation, StartOutcome, Update,
    local, remote, start_operation,
};
use crate::{
    media::codec::{self, DirectionState},
    peer_session,
};

struct PendingStop {
    session_id: peer_session::SessionId,
    reply: oneshot::Sender<Result<(), String>>,
}

pub(super) struct Runtime {
    command_rx: mpsc::Receiver<Command>,
    commands_closed: bool,
    config_rx: watch::Receiver<Config>,
    session_snapshots: watch::Receiver<peer_session::Snapshot>,
    session_events: tokio::sync::broadcast::Receiver<peer_session::Event>,
    codec_snapshots: watch::Receiver<codec::Snapshot>,
    sessions: peer_session::ServiceHandle,
    output: Output,
    local: Option<local::Controller>,
    start_operation: Option<StartOperation>,
    start_deadline: Option<tokio::time::Instant>,
    pending_stops: Vec<PendingStop>,
    encoder_restart_required: bool,
    remote: remote::Controller,
    reconciler: Reconciler,
    shutdown_rx: watch::Receiver<Option<tokio::time::Instant>>,
}

impl Runtime {
    pub(super) fn new(
        command_rx: mpsc::Receiver<Command>,
        config_rx: watch::Receiver<Config>,
        sessions: peer_session::ServiceHandle,
        codecs: codec::ServiceHandle,
        output: Output,
        shutdown_rx: watch::Receiver<Option<tokio::time::Instant>>,
    ) -> Self {
        let codec_snapshots = codecs.subscribe();
        let initial_codec = codec_snapshots.borrow().clone();
        let encoder_restart_required =
            matches!(initial_codec.encode, DirectionState::RestartRequired(_));
        let decoder_restart_required =
            matches!(initial_codec.decode, DirectionState::RestartRequired(_));
        let mut local = local::Controller::new(output.clone(), codecs.clone());
        let mut remote = remote::Controller::new(output.clone(), codecs);
        if encoder_restart_required {
            local.require_restart();
            output.publish(Update::CodecRestartRequired { direction: CodecDirection::Encoder });
        }
        if decoder_restart_required {
            remote.require_restart();
            output.publish(Update::CodecRestartRequired { direction: CodecDirection::Decoder });
        }
        Self {
            command_rx,
            commands_closed: false,
            config_rx,
            session_snapshots: sessions.subscribe(),
            session_events: sessions.events(),
            codec_snapshots,
            sessions: sessions.clone(),
            output: output.clone(),
            local: Some(local),
            start_operation: None,
            start_deadline: None,
            pending_stops: Vec::new(),
            encoder_restart_required,
            remote,
            reconciler: Reconciler::new(sessions, output),
            shutdown_rx,
        }
    }

    pub(super) async fn run(mut self) -> Result<(), String> {
        let mut reconcile_tick = tokio::time::interval(Duration::from_millis(250));
        reconcile_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let (deadline, outcome) = loop {
            tokio::select! {
                biased;
                changed = self.shutdown_rx.changed() => {
                    let deadline = if changed.is_ok() {
                        self.shutdown_rx.borrow_and_update().unwrap_or(tokio::time::Instant::now())
                    } else {
                        tokio::time::Instant::now()
                    };
                    break (deadline, Ok(()));
                }
                completed = wait_for_start(&mut self.start_operation), if self.start_operation.is_some() => {
                    if let Err(error) = self.complete_start(completed, true).await {
                        break (
                            tokio::time::Instant::now() + super::PIPELINE_SHUTDOWN_TIMEOUT,
                            Err(error),
                        );
                    }
                    let snapshot = self.sessions.snapshot();
                    self.reconcile(&snapshot).await;
                }
                _ = wait_for_deadline(self.start_deadline), if self.start_deadline.is_some() => {
                    self.start_deadline = None;
                    if let Some(operation) = self.start_operation.as_mut() {
                        let reason = "screen-share startup did not complete in time".to_owned();
                        operation.fail(reason.clone());
                        operation.respond(Err(reason));
                    }
                }
                command = self.command_rx.recv(), if !self.commands_closed => {
                    match command {
                        Some(command) => self.handle_command(command).await,
                        None => self.commands_closed = true,
                    }
                }
                changed = self.session_snapshots.changed() => {
                    if changed.is_err() {
                        break (
                            tokio::time::Instant::now() + super::PIPELINE_SHUTDOWN_TIMEOUT,
                            Err("peer-session snapshot source closed".into()),
                        );
                    }
                    let snapshot = self.session_snapshots.borrow_and_update().clone();
                    self.cancel_start_if_session_is_not_live(&snapshot);
                    self.reconcile(&snapshot).await;
                }
                event = self.session_events.recv() => match event {
                    Ok(_) => {
                        let snapshot = self.sessions.snapshot();
                        self.cancel_start_if_session_is_not_live(&snapshot);
                        self.reconcile(&snapshot).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "peer-session events lagged; reconciling snapshot");
                        let snapshot = self.sessions.snapshot();
                        self.cancel_start_if_session_is_not_live(&snapshot);
                        self.reconcile(&snapshot).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break (
                            tokio::time::Instant::now() + super::PIPELINE_SHUTDOWN_TIMEOUT,
                            Err("peer-session event source closed".into()),
                        );
                    }
                },
                changed = self.codec_snapshots.changed() => {
                    if changed.is_err() {
                        break (
                            tokio::time::Instant::now() + super::PIPELINE_SHUTDOWN_TIMEOUT,
                            Err("codec snapshot source closed".into()),
                        );
                    }
                    let snapshot = self.codec_snapshots.borrow_and_update().clone();
                    self.apply_codec_health(&snapshot).await;
                }
                _ = reconcile_tick.tick() => {
                    let snapshot = self.sessions.snapshot();
                    self.cancel_start_if_session_is_not_live(&snapshot);
                    self.reconcile(&snapshot).await;
                }
            }
        };

        let operation_clean = self.shutdown_start_until(deadline).await;
        self.fail_pending_stops("screen-share service is shutting down");

        // Both controllers receive the same absolute deadline, keeping total
        // shutdown time independent of the number of active pipelines.
        let local_shutdown = async {
            match self.local.as_mut() {
                Some(local) => local.shutdown_until(deadline).await,
                None => false,
            }
        };
        let (local_clean, remote_clean) =
            tokio::join!(local_shutdown, self.remote.shutdown_until(deadline));
        match outcome {
            Ok(()) if operation_clean && local_clean && remote_clean => Ok(()),
            Ok(()) => {
                Err("one or more screen-share operations or pipelines did not stop cleanly".into())
            }
            Err(error) => Err(error),
        }
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::BeginSelection { selection, reply } => {
                let key = selection.key();
                let session_id = key.session_id();
                let result = if self.start_operation.is_some() {
                    Err("a screen-share start is already in progress".into())
                } else if !self.session_permits_start(session_id) {
                    Err("the peer session is not connected or already has a local share".into())
                } else {
                    self.local
                        .as_mut()
                        .expect("local controller is available without a start operation")
                        .begin_selection(key.clone())
                        .await
                };
                let installed = result.is_ok();
                if reply.send(result).is_err()
                    && installed
                    && let Some(local) = self.local.as_mut()
                {
                    let _ = local.cancel_selection(&key).await;
                }
            }
            Command::CancelSelection { selection, reply } => {
                let key = selection.key();
                let result = if self
                    .start_operation
                    .as_ref()
                    .is_some_and(|operation| operation.selection() == &key)
                {
                    self.start_operation.as_ref().expect("start operation present").cancel();
                    Ok(())
                } else if let Some(local) = self.local.as_mut() {
                    local.cancel_selection(&key).await
                } else {
                    Err("screen-share selection is no longer current".into())
                };
                let _ = reply.send(result);
            }
            Command::FailSelection { selection, reason, reply } => {
                let key = selection.key();
                let result = if self
                    .start_operation
                    .as_ref()
                    .is_some_and(|operation| operation.selection() == &key)
                {
                    self.start_operation.as_ref().expect("start operation present").fail(reason);
                    Ok(())
                } else if let Some(local) = self.local.as_mut() {
                    local.fail_selection(&key, reason).await
                } else {
                    Err("screen-share selection is no longer current".into())
                };
                let _ = reply.send(result);
            }
            Command::StartScreenShare { selection, item, reply } => {
                let key = selection.key();
                if self.start_operation.is_some() {
                    let _ = reply.send(Err("a screen-share start is already in progress".into()));
                    return;
                }
                if !self.session_permits_start(key.session_id()) {
                    let reason =
                        "the peer session is not connected or already has a local share".to_owned();
                    if let Some(local) = self.local.as_mut() {
                        let _ = local.fail_selection(&key, reason.clone()).await;
                    }
                    let _ = reply.send(Err(reason));
                    return;
                }
                let local = self
                    .local
                    .as_mut()
                    .expect("local controller is available without a start operation");
                if let Err(error) = local.begin_start(&key).await {
                    let _ = local.fail_selection(&key, error.clone()).await;
                    let _ = reply.send(Err(error));
                    return;
                }
                let local = self.local.take().expect("validated local controller disappeared");
                let deadline = tokio::time::Instant::now() + super::START_OPERATION_TIMEOUT;
                self.start_deadline = Some(deadline);
                self.start_operation = Some(StartOperation::spawn(
                    local,
                    key,
                    item,
                    self.sessions.clone(),
                    self.config_rx.borrow().clone(),
                    deadline,
                    reply,
                ));
            }
            Command::StopScreenShare { session_id, reply } => {
                if let Some(operation) = self.start_operation.as_ref() {
                    if operation.selection().session_id() == session_id {
                        operation.cancel();
                    }
                    self.pending_stops.push(PendingStop { session_id, reply });
                } else {
                    let result = self.stop_now(session_id).await;
                    let _ = reply.send(result);
                }
            }
        }
    }

    async fn complete_start(
        &mut self,
        completed: Result<StartOutcome, tokio::task::JoinError>,
        process_stops: bool,
    ) -> Result<(), String> {
        let mut operation = self.start_operation.take().expect("completed start operation exists");
        self.start_deadline = None;
        if completed.is_err() {
            operation.cancel();
        }
        operation.finish();
        let selection = operation.selection().clone();
        let StartOutcome { mut local, mut result } = match completed {
            Ok(outcome) => outcome,
            Err(error) => {
                let reason = format!("screen-share start task failed: {error}");
                operation.respond(Err(reason.clone()));
                self.fail_pending_stops(&reason);
                return Err(reason);
            }
        };

        if let Some(reason) = operation.take_failure() {
            selection.cancel();
            if result.is_ok() {
                let binding = local.abort_start(&selection).await;
                if let Some(binding) = binding {
                    start_operation::stop_share(
                        &self.sessions,
                        binding.session_id(),
                        binding.share_id(),
                    )
                    .await;
                }
            }
            local.report_failure(selection.session_id(), reason.clone()).await;
            result = Err(reason);
        }

        if selection.is_cancelled() && result.is_ok() {
            let binding = local.abort_start(&selection).await;
            if let Some(binding) = binding {
                start_operation::stop_share(
                    &self.sessions,
                    binding.session_id(),
                    binding.share_id(),
                )
                .await;
            }
            result = Err("screen-share start was cancelled".into());
        }

        let delivered = operation.respond(result.clone());
        if !delivered && result.is_ok() {
            selection.cancel();
            let binding = local.abort_start(&selection).await;
            if let Some(binding) = binding {
                start_operation::stop_share(
                    &self.sessions,
                    binding.session_id(),
                    binding.share_id(),
                )
                .await;
            }
        }
        if self.encoder_restart_required {
            local.require_restart();
        }
        self.local = Some(local);

        if process_stops {
            self.process_pending_stops().await;
        } else {
            self.fail_pending_stops("screen-share service is shutting down");
        }
        Ok(())
    }

    async fn process_pending_stops(&mut self) {
        let stops = std::mem::take(&mut self.pending_stops);
        for stop in stops {
            let result = self.stop_now(stop.session_id).await;
            let _ = stop.reply.send(result);
        }
    }

    async fn stop_now(&mut self, session_id: peer_session::SessionId) -> Result<(), String> {
        let snapshot = self.sessions.snapshot();
        let local =
            self.local.as_mut().expect("local controller is available without a start operation");
        let local_state = local.has_state_for(session_id);
        let peer_share =
            snapshot.session(session_id).and_then(|session| match session.local_share {
                peer_session::LocalShareState::Active { share_id, epoch } => {
                    Some((share_id, epoch))
                }
                peer_session::LocalShareState::Inactive => None,
            });
        if !local_state && peer_share.is_none() {
            return if snapshot.session(session_id).is_some() {
                Ok(())
            } else {
                Err("peer session was not found".into())
            };
        }
        let binding = local.request_stop(session_id, &snapshot).await;
        if let Some(binding) = binding {
            start_operation::stop_share(&self.sessions, binding.session_id(), binding.share_id())
                .await;
        }
        Ok(())
    }

    async fn shutdown_start_until(&mut self, deadline: tokio::time::Instant) -> bool {
        let Some(operation) = self.start_operation.as_ref() else {
            return true;
        };
        self.start_deadline = None;
        operation.cancel();
        let completed =
            tokio::time::timeout_at(deadline, wait_for_start(&mut self.start_operation)).await;
        match completed {
            Ok(completed) => self.complete_start(completed, false).await.is_ok(),
            Err(_) => {
                // The operation owns any in-flight synchronous WGC setup. Its
                // Drop aborts the task and detaches at the shared deadline;
                // cooperative unwinding performs capture cleanup when setup returns.
                drop(self.start_operation.take());
                false
            }
        }
    }

    fn fail_pending_stops(&mut self, reason: &str) {
        for stop in self.pending_stops.drain(..) {
            let _ = stop.reply.send(Err(reason.to_owned()));
        }
    }

    fn session_permits_start(&self, session_id: peer_session::SessionId) -> bool {
        self.sessions.snapshot().session(session_id).is_some_and(super::permits_local_share_start)
    }

    fn cancel_start_if_session_is_not_live(&self, snapshot: &peer_session::Snapshot) {
        if let Some(operation) = &self.start_operation
            && !snapshot
                .session(operation.selection().session_id())
                .is_some_and(|session| session.phase == peer_session::Phase::Connected)
        {
            operation.cancel();
        }
    }

    async fn reconcile(&mut self, snapshot: &peer_session::Snapshot) {
        let config = self.config_rx.borrow().clone();
        self.reconciler.reconcile(self.local.as_mut(), &mut self.remote, snapshot, &config).await;
        self.output.reconcile_shares(snapshot);
    }

    async fn apply_codec_health(&mut self, snapshot: &codec::Snapshot) {
        if matches!(snapshot.encode, DirectionState::RestartRequired(_)) {
            let newly_required = !self.encoder_restart_required;
            self.encoder_restart_required = true;
            if let Some(operation) = &self.start_operation {
                operation.fail("the video encoder is unavailable until Fjarsyn restarts".into());
            }
            if let Some(local) = self.local.as_mut() {
                local.require_restart();
            }
            if newly_required {
                self.emit_codec_restart(CodecDirection::Encoder).await;
            }
        }
        if matches!(snapshot.decode, DirectionState::RestartRequired(_))
            && self.remote.require_restart()
        {
            self.emit_codec_restart(CodecDirection::Decoder).await;
        }
    }

    async fn emit_codec_restart(&self, direction: CodecDirection) {
        self.output.publish(Update::CodecRestartRequired { direction });
    }
}

async fn wait_for_start(
    operation: &mut Option<StartOperation>,
) -> Result<StartOutcome, tokio::task::JoinError> {
    match operation.as_mut() {
        Some(operation) => operation.wait().await,
        None => std::future::pending().await,
    }
}

async fn wait_for_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

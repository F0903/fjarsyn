use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
    time::Instant as TokioInstant,
};

use super::{
    super::{Command, TrustBarrierOwnerId},
    RecentSessionIds,
};
use crate::{
    identity::PeerId,
    peer_session::{
        Error, Event, Limits, NetworkScope, SessionId, Sessions, TrustedPeerResolver,
        actor::{Handle as ActorHandle, TaskExit, Terminal, Update},
        negotiation,
        service::{Config, ServiceHandle},
    },
};

pub(super) struct SessionEntry {
    pub(super) handle: ActorHandle,
    pub(super) task: JoinHandle<()>,
}

impl SessionEntry {
    pub(super) fn new(handle: ActorHandle, task: JoinHandle<()>) -> Self {
        Self { handle, task }
    }
}

impl Drop for SessionEntry {
    fn drop(&mut self) {
        // JoinHandle normally detaches on drop. Aborting here makes unexpected
        // runtime cancellation fail closed instead of orphaning an actor.
        self.task.abort();
    }
}

pub(in crate::peer_session::service) struct Runtime {
    pub(super) local_peer_id: PeerId,
    pub(super) trusted_peers: Arc<dyn TrustedPeerResolver>,
    pub(super) negotiation: negotiation::Service,
    pub(super) network_scope: NetworkScope,
    pub(super) ice_servers: Vec<String>,
    pub(super) max_depacket_latency: Duration,
    pub(super) limits: Limits,
    pub(super) sessions: HashMap<SessionId, SessionEntry>,
    pub(super) peers: HashMap<PeerId, SessionId>,
    pub(super) suspended_peers: HashMap<PeerId, HashSet<TrustBarrierOwnerId>>,
    pub(super) recent_session_ids: RecentSessionIds,
    pub(super) command_rx: mpsc::Receiver<Command>,
    pub(super) incoming_rx: mpsc::Receiver<negotiation::Incoming>,
    pub(super) update_tx: mpsc::Sender<Update>,
    pub(super) update_rx: mpsc::Receiver<Update>,
    pub(super) terminal_tx: mpsc::UnboundedSender<Terminal>,
    pub(super) terminal_rx: mpsc::UnboundedReceiver<Terminal>,
    pub(super) task_exit_tx: mpsc::UnboundedSender<TaskExit>,
    pub(super) task_exit_rx: mpsc::UnboundedReceiver<TaskExit>,
    pub(super) snapshot_tx: watch::Sender<Sessions>,
    pub(super) event_tx: broadcast::Sender<Event>,
    pub(super) mandatory_event_sink: Option<mpsc::Sender<Event>>,
    pub(super) mandatory_event_sink_failed: bool,
    pub(super) listener_failure_rx: watch::Receiver<Option<Error>>,
    pub(super) shutdown_rx: watch::Receiver<Option<TokioInstant>>,
    pub(super) shutdown_complete_tx: Option<oneshot::Sender<Result<(), Error>>>,
}

impl Runtime {
    pub(in crate::peer_session::service) fn new(
        local_peer_id: PeerId,
        config: Config,
        negotiation: negotiation::Service,
        incoming_rx: mpsc::Receiver<negotiation::Incoming>,
        listener_failure_rx: watch::Receiver<Option<Error>>,
        shutdown_rx: watch::Receiver<Option<TokioInstant>>,
        shutdown_complete_tx: oneshot::Sender<Result<(), Error>>,
    ) -> (Self, ServiceHandle) {
        let (command_tx, command_rx) = mpsc::channel(config.limits.service_command_capacity.max(1));
        let (update_tx, update_rx) = mpsc::channel(config.limits.session_update_capacity.max(1));
        let (terminal_tx, terminal_rx) = mpsc::unbounded_channel();
        let (task_exit_tx, task_exit_rx) = mpsc::unbounded_channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(Sessions::default());
        let (event_tx, _) = broadcast::channel(config.limits.event_capacity.max(1));
        let operation_timeout =
            config.limits.service_operation_timeout.saturating_add(Duration::from_secs(1));
        let handle =
            ServiceHandle::new(command_tx, snapshot_rx, event_tx.clone(), operation_timeout);
        let recent_session_ids = RecentSessionIds::new(
            config.limits.signaling_max_message_age,
            config.limits.signaling_replay_capacity,
        );

        let runtime = Self {
            local_peer_id,
            trusted_peers: config.trusted_peers,
            negotiation,
            network_scope: config.network_scope,
            ice_servers: config.ice_servers,
            max_depacket_latency: config.max_depacket_latency,
            limits: config.limits,
            sessions: HashMap::new(),
            peers: HashMap::new(),
            suspended_peers: HashMap::new(),
            recent_session_ids,
            command_rx,
            incoming_rx,
            update_tx,
            update_rx,
            terminal_tx,
            terminal_rx,
            task_exit_tx,
            task_exit_rx,
            snapshot_tx,
            event_tx,
            mandatory_event_sink: config.mandatory_event_sink,
            mandatory_event_sink_failed: false,
            listener_failure_rx,
            shutdown_rx,
            shutdown_complete_tx: Some(shutdown_complete_tx),
        };
        (runtime, handle)
    }

    pub(in crate::peer_session::service) async fn run(mut self) {
        let mut snapshot_tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            let mandatory_sink = self.mandatory_event_sink.clone();
            tokio::select! {
                biased;
                changed = self.shutdown_rx.changed() => {
                    let deadline = if changed.is_ok() {
                        *self.shutdown_rx.borrow_and_update()
                    } else {
                        *self.shutdown_rx.borrow()
                    }
                    .unwrap_or_else(|| TokioInstant::now() + self.limits.shutdown_timeout);
                    self.complete_shutdown(deadline).await;
                    break;
                }
                _ = wait_for_mandatory_sink_closed(mandatory_sink) => {
                    self.fail_mandatory_event_sink();
                }
                failure = receive_listener_failure(&mut self.listener_failure_rx) => {
                    let deadline = TokioInstant::now() + self.limits.shutdown_timeout;
                    self.complete_failure(failure, deadline).await;
                    break;
                }
                command = self.command_rx.recv() => {
                    match command {
                        Some(command) => self.handle_command(command).await,
                        None => {
                            let deadline = TokioInstant::now() + self.limits.shutdown_timeout;
                            self.complete_shutdown(deadline).await;
                            break;
                        }
                    }
                }
                incoming = self.incoming_rx.recv() => {
                    if let Some(incoming) = incoming {
                        let mut shutdown_rx = self.shutdown_rx.clone();
                        tokio::select! {
                            biased;
                            _ = super::shutdown::receive_shutdown_deadline(&mut shutdown_rx) => {}
                            _ = self.handle_incoming(incoming) => {}
                        }
                    }
                }
                update = self.update_rx.recv() => {
                    if let Some(update) = update {
                        self.handle_update(update).await;
                    }
                }
                terminal = self.terminal_rx.recv() => {
                    if let Some(terminal) = terminal {
                        self.handle_terminal(terminal).await;
                    }
                }
                task_exit = self.task_exit_rx.recv() => {
                    if let Some(task_exit) = task_exit {
                        self.handle_task_exit(task_exit).await;
                    }
                }
                _ = snapshot_tick.tick() => self.publish_snapshot(),
            }
        }
    }
}

async fn wait_for_mandatory_sink_closed(sink: Option<mpsc::Sender<Event>>) {
    match sink {
        Some(sink) => sink.closed().await,
        None => std::future::pending().await,
    }
}

async fn receive_listener_failure(failure_rx: &mut watch::Receiver<Option<Error>>) -> Error {
    loop {
        if let Some(error) = failure_rx.borrow_and_update().clone() {
            return error;
        }
        if failure_rx.changed().await.is_err() {
            return Error::Listener("signaling listener stopped unexpectedly".into());
        }
    }
}

use std::sync::{Arc, Mutex};

use tokio::{
    sync::{broadcast, mpsc, watch},
    time::Instant,
};

use super::super::{
    ApplicationDataGate, Command, Config, Control, Handle, Role, TaskExit, Terminal, Update,
    restart, state_machine::StateMachine, task_supervision,
};
use crate::peer_session::{
    CloseReason, Error, LocalShareState, Phase, RemoteShareState, SessionSnapshot, ShareEpoch,
    ShareId,
    media::{encoded_video_channel, remote_video_channel},
    negotiation,
    protocol::NegotiationSignal,
    rtc::{self, Peer},
};

pub(in crate::peer_session) fn spawn(
    mut config: Config,
    update_tx: mpsc::Sender<Update>,
    terminal_tx: mpsc::UnboundedSender<Terminal>,
    task_exit_tx: mpsc::UnboundedSender<TaskExit>,
) -> (Handle, tokio::task::JoinHandle<()>) {
    let state = match config.role {
        Role::Outgoing => StateMachine::outgoing(),
        Role::Incoming => StateMachine::incoming(),
    };
    let generation = uuid::Uuid::new_v4();
    let snapshot = SessionSnapshot {
        session_id: config.session_id,
        peer_id: config.remote_peer_id.clone(),
        phase: state.phase(),
        local_share: LocalShareState::Inactive,
        remote_share: RemoteShareState::Inactive,
    };
    let (command_tx, command_rx) = mpsc::channel(config.command_capacity.max(1));
    let (snapshot_tx, snapshot_rx) = watch::channel(snapshot);
    let (encoded_video_tx, video_rx) = encoded_video_channel(config.media_capacity);
    let (active_video_tx, active_video_rx) = watch::channel(None);
    let (remote_video_tx, initial_remote_video_rx) =
        remote_video_channel(config.remote_video_capacity);
    let (rtc_event_tx, rtc_event_rx) = mpsc::channel(config.command_capacity.max(8));
    let (rtc_fatal_tx, rtc_fatal_rx) = watch::channel(None);
    let (fatal_tx, fatal_rx) = watch::channel(None);
    let (restart, restart_tx) = restart::Controller::new();

    let handle = Handle {
        session_id: config.session_id,
        generation,
        command_tx,
        restart_tx,
        snapshot_rx,
        encoded_video_tx,
        active_video_rx,
        remote_video_tx: remote_video_tx.clone(),
        initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
        fatal_tx,
    };
    let connection = config.connection.take().expect("session actor requires signaling");
    let application_data =
        ApplicationDataGate::new(config.pre_ready_data_capacity, config.max_message_bytes);
    let task_peer_id = config.remote_peer_id.clone();
    let runtime = Runtime {
        config,
        state,
        local_share: LocalShareState::Inactive,
        remote_share: RemoteShareState::Inactive,
        last_local_share_epoch: None,
        last_remote_share_epoch: None,
        connection: Some(connection),
        rtc: None,
        command_rx,
        video_rx,
        active_video_tx,
        remote_video_tx,
        rtc_event_tx,
        rtc_event_rx,
        rtc_fatal_tx,
        rtc_fatal_rx,
        fatal_rx,
        snapshot_tx,
        update_tx,
        terminal_tx,
        application_data,
        terminal_reason: None,
        generation,
        phase_started: Instant::now(),
        disconnected_since: None,
        restart,
        cleanup_deadline: None,
    };
    let task = task_supervision::spawn(
        runtime.run(),
        generation,
        handle.session_id,
        task_peer_id,
        task_exit_tx,
    );
    (handle, task)
}

pub(super) struct Runtime {
    pub(super) config: Config,
    pub(super) state: StateMachine,
    pub(super) local_share: LocalShareState,
    pub(super) remote_share: RemoteShareState,
    pub(super) last_local_share_epoch: Option<ShareEpoch>,
    pub(super) last_remote_share_epoch: Option<ShareEpoch>,
    pub(super) connection: Option<negotiation::Connection>,
    pub(super) rtc: Option<Peer>,
    pub(super) command_rx: mpsc::Receiver<Command>,
    pub(super) video_rx: mpsc::Receiver<crate::peer_session::media::OutboundVideoSample>,
    pub(super) active_video_tx: watch::Sender<Option<(ShareId, ShareEpoch)>>,
    pub(super) remote_video_tx: broadcast::Sender<crate::peer_session::media::RemoteVideoSample>,
    pub(super) rtc_event_tx: mpsc::Sender<rtc::Event>,
    pub(super) rtc_event_rx: mpsc::Receiver<rtc::Event>,
    pub(super) rtc_fatal_tx: watch::Sender<Option<String>>,
    pub(super) rtc_fatal_rx: watch::Receiver<Option<String>>,
    pub(super) fatal_rx: watch::Receiver<Option<Control>>,
    pub(super) snapshot_tx: watch::Sender<SessionSnapshot>,
    pub(super) update_tx: mpsc::Sender<Update>,
    pub(super) terminal_tx: mpsc::UnboundedSender<Terminal>,
    pub(super) application_data: ApplicationDataGate,
    pub(super) terminal_reason: Option<CloseReason>,
    pub(super) generation: uuid::Uuid,
    pub(super) phase_started: Instant,
    pub(super) disconnected_since: Option<Instant>,
    pub(super) restart: restart::Controller,
    pub(super) cleanup_deadline: Option<Instant>,
}

impl Runtime {
    async fn run(mut self) {
        self.publish_snapshot().await;
        let mut timeout_check = tokio::time::interval(std::time::Duration::from_millis(250));

        if self.config.role == Role::Outgoing
            && let Err(error) = self.send_signal(NegotiationSignal::Request {}).await
        {
            self.terminal_reason =
                Some(CloseReason::ConnectionFailed { reason: error.to_string() });
        }

        while self.terminal_reason.is_none() {
            tokio::select! {
                biased;
                changed = self.fatal_rx.changed() => {
                    if changed.is_err() {
                        self.fail("peer-session fatal control channel closed".into());
                    } else {
                        let control = self.fatal_rx.borrow_and_update().clone();
                        match control {
                            Some(Control::Fail(reason)) => self.fail(reason),
                            Some(Control::TrustRevoked { deadline }) => {
                                self.cleanup_deadline = Some(deadline);
                                self.terminal_reason = Some(CloseReason::TrustRevoked);
                            }
                            Some(Control::Shutdown { deadline }) => {
                                self.cleanup_deadline = Some(deadline);
                                self.terminal_reason = Some(CloseReason::ServiceShutdown);
                            }
                            None => {}
                        }
                    }
                }
                changed = self.rtc_fatal_rx.changed(), if self.rtc.is_some() => {
                    if changed.is_err() {
                        self.fail("WebRTC fatal event channel closed".into());
                    } else {
                        let reason = self.rtc_fatal_rx.borrow_and_update().clone();
                        if let Some(reason) = reason {
                            self.fail(reason);
                        }
                    }
                }
                restart_event = self.restart.next_event(self.state.phase()) => {
                    match restart_event {
                        restart::Event::DeadlineElapsed => self.fail("ICE restart timed out".into()),
                        restart::Event::RejectionTaskFailed => {
                            self.fail("restart rejection cleanup task failed".into());
                        }
                        restart::Event::Attachment(attachment) => {
                            self.attach_restart(attachment.generation, attachment.connection).await;
                        }
                        restart::Event::DialCompleted(result) => {
                            self.handle_restart_dial_result(result).await;
                        }
                        restart::Event::DialTaskFailed => {
                            self.fail("ICE restart signaling task stopped".into());
                        }
                    }
                }
                signal = async {
                    match self.connection.as_mut() {
                        Some(connection) => connection.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match signal {
                        Some(Ok(signal)) => self.handle_signal(signal).await,
                        Some(Err(error)) => self.fail_error(error),
                        None if self.state.phase() != Phase::Connected => {
                            self.terminal_reason = Some(CloseReason::SignalingLost);
                        }
                        None => {}
                    }
                }
                event = self.rtc_event_rx.recv(), if self.rtc.is_some() => {
                    match event {
                        Some(event) => self.handle_rtc_event(event).await,
                        None => self.fail("WebRTC event channel closed".into()),
                    }
                }
                command = self.command_rx.recv() => {
                    match command {
                        Some(command) => self.handle_command(command).await,
                        None => self.terminal_reason = Some(CloseReason::ServiceShutdown),
                    }
                }
                tagged = self.video_rx.recv() => {
                    let Some(tagged) = tagged else { continue };
                    if self.local_share == (LocalShareState::Active {
                        share_id: tagged.share_id,
                        epoch: tagged.epoch,
                    })
                        && self.state.phase() == Phase::Connected
                        && let Some(rtc) = self.rtc.as_ref()
                        && let Err(error) = rtc.write_video(tagged).await
                    {
                        self.fail(error.to_string());
                    }
                }
                _ = timeout_check.tick() => {
                    let timeout = match self.state.phase() {
                        Phase::Requesting | Phase::Incoming => {
                            Some(self.config.request_timeout)
                        }
                        Phase::Negotiating => Some(self.config.negotiation_timeout),
                        Phase::Connected
                        | Phase::Reconnecting
                        | Phase::Disconnecting => None,
                    };
                    if timeout.is_some_and(|timeout| self.phase_started.elapsed() >= timeout) {
                        self.fail(format!("{} phase timed out", self.state.phase().name()));
                    }
                    if self.state.phase() == Phase::Connected
                        && self.disconnected_since.is_some_and(|since| {
                            disconnect_grace_expired(
                                since,
                                Instant::now(),
                                self.config.disconnected_grace,
                            )
                        })
                        && let Err(error) = self.begin_ice_restart().await
                    {
                        self.fail_error(error);
                    }
                }
            }
        }

        reject_queued_session_commands(&mut self.command_rx);

        let cleanup_deadline =
            self.cleanup_deadline.unwrap_or_else(|| Instant::now() + self.config.cleanup_timeout);
        self.restart.shutdown(cleanup_deadline).await;
        let connection = self.connection.take();
        let rtc = self.rtc.take();
        tokio::join!(
            async move {
                if let Some(connection) = connection {
                    connection.shutdown_until(cleanup_deadline).await;
                }
            },
            async move {
                if let Some(rtc) = rtc {
                    rtc.shutdown_until(cleanup_deadline).await;
                }
            },
        );
        let reason = self.terminal_reason.unwrap_or(CloseReason::ServiceShutdown);
        let _ = self.terminal_tx.send(Terminal {
            generation: self.generation,
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            reason,
        });
    }
}

fn disconnect_grace_expired(since: Instant, now: Instant, grace: std::time::Duration) -> bool {
    now.duration_since(since) >= grace
}

fn reject_queued_session_commands(command_rx: &mut mpsc::Receiver<Command>) {
    command_rx.close();
    while let Ok(command) = command_rx.try_recv() {
        command.reply_error(Error::ServiceStopped);
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use super::*;

    #[test]
    fn transient_disconnect_has_a_bounded_recovery_window() {
        let since = Instant::now();
        let grace = std::time::Duration::from_secs(5);

        assert!(
            !disconnect_grace_expired(since, since + std::time::Duration::from_secs(4), grace,)
        );
        assert!(disconnect_grace_expired(since, since + grace, grace));
    }

    #[tokio::test]
    async fn queued_unstarted_commands_receive_a_definitive_shutdown_error() {
        let (command_tx, mut command_rx) = mpsc::channel(1);
        let (reply_tx, reply_rx) = oneshot::channel();
        command_tx.send(Command::Accept(reply_tx)).await.unwrap();

        reject_queued_session_commands(&mut command_rx);

        assert_eq!(reply_rx.await.unwrap(), Err(Error::ServiceStopped));
        assert!(command_rx.is_closed());
    }
}

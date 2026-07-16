#[cfg(test)]
mod tests;

use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

pub use crate::communication::messaging::{
    ConversationMessage, ConversationSummary, MessageDirection, MessageStatus, MessagingEvent,
    MessagingSnapshot,
};
use crate::{
    Error as CoreError,
    communication::messaging::{
        MessageRecordError, build_messaging_snapshot, with_upserted_message,
    },
    database::MessageModel,
    peer_session::{
        MessageId, PeerId, PeerSessionError, PeerSessionEvent, PeerSessionPhase,
        PeerSessionServiceHandle, PeerSessionServiceSnapshot, SessionId,
    },
    repositories::MessagesStore,
};

const COMMAND_CAPACITY: usize = 32;
pub const SESSION_EVENT_CAPACITY: usize = 256;
const EVENT_CAPACITY: usize = 256;
// This is the application chat limit as well as the default peer-session protocol limit.
const MAX_MESSAGE_BODY_BYTES: usize = 12 * 1024;

#[derive(Debug, Clone)]
pub struct MessagingServiceLimits {
    /// Maximum time an accepted command may wait in the actor queue before it
    /// is rejected without side effects.
    pub command_start_timeout: Duration,
    /// One aggregate deadline for draining mandatory events and joining the
    /// actor during explicit shutdown.
    pub shutdown_timeout: Duration,
}

impl Default for MessagingServiceLimits {
    fn default() -> Self {
        Self {
            command_start_timeout: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(10),
        }
    }
}

pub struct MessagingServiceConfig {
    pub repository: Arc<dyn MessagesStore>,
    pub sessions: PeerSessionServiceHandle,
    /// The single bounded, mandatory event stream owned by the peer-session
    /// service. Application composition creates this channel before either
    /// service starts and gives the sender to `PeerSessionServiceConfig`.
    pub session_events: mpsc::Receiver<PeerSessionEvent>,
    pub limits: MessagingServiceLimits,
}

#[derive(thiserror::Error, Debug)]
pub enum MessagingError {
    #[error("database error: {0}")]
    Database(#[from] CoreError),
    #[error("peer session error: {0}")]
    Session(#[from] PeerSessionError),
    #[error(transparent)]
    MessageRecord(#[from] MessageRecordError),
    #[error("message body cannot be empty")]
    EmptyBody,
    #[error("message body exceeds the {max} byte limit")]
    MessageTooLarge { max: usize },
    #[error("session {session_id} belongs to {actual_peer}, not {expected_peer}")]
    SessionPeerMismatch { session_id: SessionId, expected_peer: PeerId, actual_peer: PeerId },
    #[error("session {session_id} is not connected")]
    SessionNotConnected { session_id: SessionId },
    #[error("message {message_id} could not transition to {status:?}")]
    MissingMessageTransition { message_id: MessageId, status: MessageStatus },
    #[error("the messaging service has stopped")]
    ServiceStopped,
    #[error("the messaging service command queue is full")]
    ServiceBusy,
    #[error("the messaging command expired before execution")]
    CommandExpired,
    #[error("the messaging service is stopping")]
    ServiceStopping,
    #[error("the messaging command response was dropped")]
    ResponseDropped,
    #[error("messaging shutdown timed out")]
    ShutdownTimeout,
    #[error("the messaging task failed: {0}")]
    TaskFailed(String),
}

pub struct MessagingService {
    handle: MessagingServiceHandle,
    shutdown_tx: watch::Sender<bool>,
    shutdown_timeout: Duration,
    task: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl fmt::Debug for MessagingService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessagingService").finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct MessagingServiceHandle {
    command_tx: mpsc::Sender<Command>,
    snapshot_rx: watch::Receiver<MessagingSnapshot>,
    event_tx: broadcast::Sender<MessagingEvent>,
    command_start_timeout: Duration,
}

impl fmt::Debug for MessagingServiceHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessagingServiceHandle").finish_non_exhaustive()
    }
}

impl MessagingService {
    pub async fn start(config: MessagingServiceConfig) -> Result<Self, MessagingError> {
        let session_transport: Arc<dyn SessionMessaging> =
            Arc::new(PeerSessionMessaging(config.sessions));
        Self::start_with_transport_and_limits(
            config.repository,
            session_transport,
            config.session_events,
            config.limits,
        )
        .await
    }

    #[cfg(test)]
    async fn start_with_transport(
        repository: Arc<dyn MessagesStore>,
        sessions: Arc<dyn SessionMessaging>,
        session_events: mpsc::Receiver<PeerSessionEvent>,
    ) -> Result<Self, MessagingError> {
        Self::start_with_transport_and_limits(
            repository,
            sessions,
            session_events,
            MessagingServiceLimits::default(),
        )
        .await
    }

    async fn start_with_transport_and_limits(
        repository: Arc<dyn MessagesStore>,
        sessions: Arc<dyn SessionMessaging>,
        session_events: mpsc::Receiver<PeerSessionEvent>,
        limits: MessagingServiceLimits,
    ) -> Result<Self, MessagingError> {
        // A process can stop after SCTP accepted a frame but before the `Sent`
        // transition commits. Never retry it into a later session, and never
        // make the false claim that delivery definitely failed.
        repository.mark_all_pending_unknown().await?;
        let initial_snapshot = load_snapshot(&repository).await?;

        let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (snapshot_tx, snapshot_rx) = watch::channel(initial_snapshot.clone());
        let (event_tx, _) = broadcast::channel(EVENT_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let actor = MessagingActor {
            repository,
            sessions,
            commands: command_rx,
            session_events,
            session_events_open: true,
            snapshot: initial_snapshot,
            snapshot_tx,
            event_tx: event_tx.clone(),
            shutdown_rx,
        };
        let task = tokio::spawn(actor.run());

        Ok(Self {
            handle: MessagingServiceHandle {
                command_tx,
                snapshot_rx,
                event_tx,
                command_start_timeout: limits.command_start_timeout,
            },
            shutdown_tx,
            shutdown_timeout: limits.shutdown_timeout,
            task: Some(task),
        })
    }

    pub fn handle(&self) -> MessagingServiceHandle {
        self.handle.clone()
    }

    pub async fn shutdown(mut self) -> Result<(), MessagingError> {
        let _ = self.shutdown_tx.send(true);
        let Some(mut task) = self.task.take() else { return Ok(()) };
        match tokio::time::timeout(self.shutdown_timeout, &mut task).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(reason))) => Err(MessagingError::TaskFailed(reason)),
            Ok(Err(error)) => Err(MessagingError::TaskFailed(error.to_string())),
            Err(_) => {
                task.abort();
                let _ = task.await;
                Err(MessagingError::ShutdownTimeout)
            }
        }
    }
}

impl Drop for MessagingService {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl MessagingServiceHandle {
    pub fn snapshot(&self) -> MessagingSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<MessagingSnapshot> {
        self.snapshot_rx.clone()
    }

    pub fn events(&self) -> broadcast::Receiver<MessagingEvent> {
        self.event_tx.subscribe()
    }

    pub async fn send_message(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        body: String,
    ) -> Result<MessageId, MessagingError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.enqueue(Command::SendMessage {
            deadline: Instant::now() + self.command_start_timeout,
            session_id,
            peer_id,
            body,
            response_tx,
        })?;
        response_rx.await.map_err(|_| MessagingError::ResponseDropped)?
    }

    pub async fn refresh(&self) -> Result<(), MessagingError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.enqueue(Command::Refresh {
            deadline: Instant::now() + self.command_start_timeout,
            response_tx,
        })?;
        response_rx.await.map_err(|_| MessagingError::ResponseDropped)?
    }

    fn enqueue(&self, command: Command) -> Result<(), MessagingError> {
        self.command_tx.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => MessagingError::ServiceBusy,
            mpsc::error::TrySendError::Closed(_) => MessagingError::ServiceStopped,
        })
    }
}

enum Command {
    SendMessage {
        deadline: Instant,
        session_id: SessionId,
        peer_id: PeerId,
        body: String,
        response_tx: oneshot::Sender<Result<MessageId, MessagingError>>,
    },
    Refresh {
        deadline: Instant,
        response_tx: oneshot::Sender<Result<(), MessagingError>>,
    },
}

struct MessagingActor {
    repository: Arc<dyn MessagesStore>,
    sessions: Arc<dyn SessionMessaging>,
    commands: mpsc::Receiver<Command>,
    session_events: mpsc::Receiver<PeerSessionEvent>,
    session_events_open: bool,
    snapshot: MessagingSnapshot,
    snapshot_tx: watch::Sender<MessagingSnapshot>,
    event_tx: broadcast::Sender<MessagingEvent>,
    shutdown_rx: watch::Receiver<bool>,
}

impl MessagingActor {
    async fn run(mut self) -> Result<(), String> {
        loop {
            tokio::select! {
                biased;
                changed = self.shutdown_rx.changed() => {
                    if changed.is_err() || *self.shutdown_rx.borrow_and_update() {
                        return self.shutdown().await.map_err(|error| error.to_string());
                    }
                }
                command = self.commands.recv() => {
                    match command {
                        Some(Command::SendMessage {
                            deadline,
                            session_id,
                            peer_id,
                            body,
                            response_tx,
                        }) => {
                            if Instant::now() >= deadline {
                                let _ = response_tx.send(Err(MessagingError::CommandExpired));
                                continue;
                            }
                            let result = self.send_message(session_id, peer_id, body).await;
                            let fatal = fatal_messaging_error(&result);
                            let _ = response_tx.send(result);
                            if let Some(reason) = fatal {
                                return Err(reason);
                            }
                        }
                        Some(Command::Refresh { deadline, response_tx }) => {
                            if Instant::now() >= deadline {
                                let _ = response_tx.send(Err(MessagingError::CommandExpired));
                                continue;
                            }
                            let result = self.refresh().await;
                            let fatal = fatal_messaging_error(&result);
                            let _ = response_tx.send(result);
                            if let Some(reason) = fatal {
                                return Err(reason);
                            }
                        }
                        None => return Ok(()),
                    }
                }
                event = self.session_events.recv(), if self.session_events_open => {
                    match event {
                        Some(event) => {
                            if let Err(error) = self.handle_session_event(event).await {
                                if is_fatal_messaging_error(&error) {
                                    return Err(error.to_string());
                                }
                                tracing::warn!(%error, "peer-session messaging event could not be completed");
                            }
                        }
                        None => self.session_events_open = false,
                    }
                }
            }
        }
    }

    async fn shutdown(&mut self) -> Result<(), MessagingError> {
        self.commands.close();
        while let Some(command) = self.commands.recv().await {
            reject_stopping(command);
        }

        // Closing the receiver rejects any late producer while still allowing
        // every event already accepted by the mandatory bounded channel to be
        // drained before shutdown completes.
        self.session_events.close();
        while let Some(event) = self.session_events.recv().await {
            if let Err(error) = self.handle_session_event(event).await {
                if is_fatal_messaging_error(&error) {
                    return Err(error);
                }
                tracing::warn!(%error, "peer-session messaging event could not complete during shutdown");
            }
        }
        self.session_events_open = false;
        Ok(())
    }

    async fn send_message(
        &mut self,
        session_id: SessionId,
        peer_id: PeerId,
        body: String,
    ) -> Result<MessageId, MessagingError> {
        let body = validate_body(body)?;
        self.validate_connected_session(session_id, &peer_id)?;

        let message_id = MessageId::new();
        let created_at = Utc::now();
        let pending = self
            .repository
            .create_outgoing(session_id, message_id, peer_id.clone(), body.clone(), created_at)
            .await?;
        self.publish_model(pending)?;
        self.emit(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() });

        if let Err(error) =
            self.sessions.send_message(session_id, message_id, body, created_at).await
        {
            let status = if error == PeerSessionError::OutcomeUnknown {
                MessageStatus::Unknown
            } else {
                MessageStatus::Failed
            };
            let model = if status == MessageStatus::Unknown {
                self.repository.mark_unknown(session_id, peer_id.clone(), message_id).await?
            } else {
                self.repository.mark_failed(session_id, peer_id.clone(), message_id).await?
            }
            .ok_or(MessagingError::MissingMessageTransition { message_id, status })?;
            self.publish_model(model)?;
            self.emit(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() });
            self.emit(MessagingEvent::MessageStatusChanged {
                session_id,
                peer_id,
                message_id,
                status,
            });
            return Err(error.into());
        }

        let sent =
            self.repository.mark_sent(session_id, peer_id.clone(), message_id).await?.ok_or(
                MessagingError::MissingMessageTransition {
                    message_id,
                    status: MessageStatus::Sent,
                },
            )?;
        self.publish_model(sent)?;
        self.emit(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() });
        self.emit(MessagingEvent::MessageStatusChanged {
            session_id,
            peer_id,
            message_id,
            status: MessageStatus::Sent,
        });
        Ok(message_id)
    }

    async fn refresh(&mut self) -> Result<(), MessagingError> {
        self.snapshot = load_snapshot(&self.repository).await?;
        self.snapshot_tx.send_replace(self.snapshot.clone());
        Ok(())
    }

    fn validate_connected_session(
        &self,
        session_id: SessionId,
        expected_peer: &PeerId,
    ) -> Result<(), MessagingError> {
        let sessions = self.sessions.snapshot();
        let session = sessions
            .session(session_id)
            .ok_or(MessagingError::SessionNotConnected { session_id })?;
        if &session.peer_id != expected_peer {
            return Err(MessagingError::SessionPeerMismatch {
                session_id,
                expected_peer: expected_peer.clone(),
                actual_peer: session.peer_id.clone(),
            });
        }
        if session.phase != PeerSessionPhase::Connected {
            return Err(MessagingError::SessionNotConnected { session_id });
        }
        Ok(())
    }

    async fn handle_session_event(
        &mut self,
        event: PeerSessionEvent,
    ) -> Result<(), MessagingError> {
        match event {
            PeerSessionEvent::MessageReceived {
                session_id,
                peer_id,
                message_id,
                body,
                sent_at,
            } => {
                let body = validate_body(body)?;
                let received_at = Utc::now();
                let inserted = self
                    .repository
                    .create_incoming_if_missing(
                        session_id,
                        message_id,
                        peer_id.clone(),
                        body.clone(),
                        sent_at,
                        received_at,
                    )
                    .await?;

                if let Some(message) = inserted {
                    self.publish_model(message)?;
                    self.emit(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() });
                    self.emit(MessagingEvent::IncomingMessage {
                        session_id,
                        peer_id: peer_id.clone(),
                        message_id,
                        body,
                    });
                }

                // Duplicate delivery is acknowledged again, but is never persisted twice.
                self.sessions.send_receipt(session_id, message_id, received_at).await?;
            }
            PeerSessionEvent::MessageReceiptReceived {
                session_id,
                peer_id,
                message_id,
                received_at,
            } => {
                if let Some(message) = self
                    .repository
                    .mark_delivered(session_id, peer_id.clone(), message_id, received_at)
                    .await?
                {
                    self.publish_model(message)?;
                    self.emit(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() });
                    self.emit(MessagingEvent::MessageStatusChanged {
                        session_id,
                        peer_id,
                        message_id,
                        status: MessageStatus::Delivered,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn publish_model(&mut self, model: MessageModel) -> Result<(), MessagingError> {
        let message = ConversationMessage::try_from(model)?;
        self.snapshot = with_upserted_message(&self.snapshot, message);
        self.snapshot_tx.send_replace(self.snapshot.clone());
        Ok(())
    }

    fn emit(&self, event: MessagingEvent) {
        let _ = self.event_tx.send(event);
    }
}

fn reject_stopping(command: Command) {
    match command {
        Command::SendMessage { response_tx, .. } => {
            let _ = response_tx.send(Err(MessagingError::ServiceStopping));
        }
        Command::Refresh { response_tx, .. } => {
            let _ = response_tx.send(Err(MessagingError::ServiceStopping));
        }
    }
}

fn fatal_messaging_error<T>(result: &Result<T, MessagingError>) -> Option<String> {
    result.as_ref().err().filter(|error| is_fatal_messaging_error(error)).map(ToString::to_string)
}

fn is_fatal_messaging_error(error: &MessagingError) -> bool {
    matches!(
        error,
        MessagingError::Database(_)
            | MessagingError::MessageRecord(_)
            | MessagingError::MissingMessageTransition { .. }
    )
}

fn validate_body(body: String) -> Result<String, MessagingError> {
    let body = body.trim().to_owned();
    if body.is_empty() {
        return Err(MessagingError::EmptyBody);
    }
    if body.len() > MAX_MESSAGE_BODY_BYTES {
        return Err(MessagingError::MessageTooLarge { max: MAX_MESSAGE_BODY_BYTES });
    }
    Ok(body)
}

async fn load_snapshot(
    repository: &Arc<dyn MessagesStore>,
) -> Result<MessagingSnapshot, MessagingError> {
    let messages = repository
        .list()
        .await?
        .into_iter()
        .map(ConversationMessage::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(build_messaging_snapshot(messages))
}

#[async_trait]
trait SessionMessaging: Send + Sync {
    fn snapshot(&self) -> PeerSessionServiceSnapshot;

    async fn send_message(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    ) -> Result<(), PeerSessionError>;

    async fn send_receipt(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), PeerSessionError>;
}

struct PeerSessionMessaging(PeerSessionServiceHandle);

#[async_trait]
impl SessionMessaging for PeerSessionMessaging {
    fn snapshot(&self) -> PeerSessionServiceSnapshot {
        self.0.snapshot()
    }

    async fn send_message(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    ) -> Result<(), PeerSessionError> {
        self.0.send_message(session_id, message_id, body, sent_at).await
    }

    async fn send_receipt(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), PeerSessionError> {
        self.0.send_receipt(session_id, message_id, received_at).await
    }
}

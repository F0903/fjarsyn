//! Serialized messaging actor, its commands, and session-event processing.

use std::{sync::Arc, time::Instant};

use chrono::Utc;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::{
    COMMAND_CAPACITY, ConversationMessage, Conversations, EVENT_CAPACITY, Error, Event,
    MessageRecord, MessageStatus, Store,
    conversations::{build, with_upserted_message},
    transport::SessionMessaging,
};
use crate::{
    identity::PeerId,
    peer_session::{self, MessageId, Phase, SessionId},
};

// This is the application chat limit as well as the default peer-session protocol limit.
const MAX_MESSAGE_BODY_BYTES: usize = 12 * 1024;

pub(in crate::messaging) enum Command {
    SendMessage {
        deadline: Instant,
        session_id: SessionId,
        peer_id: PeerId,
        body: String,
        response_tx: oneshot::Sender<Result<MessageId, Error>>,
    },
    Refresh {
        deadline: Instant,
        response_tx: oneshot::Sender<Result<(), Error>>,
    },
}

pub(in crate::messaging) struct Actor {
    store: Arc<dyn Store>,
    sessions: Arc<dyn SessionMessaging>,
    commands: mpsc::Receiver<Command>,
    session_events: mpsc::Receiver<peer_session::Event>,
    session_events_open: bool,
    snapshot: Conversations,
    snapshot_tx: watch::Sender<Conversations>,
    event_tx: broadcast::Sender<Event>,
    shutdown_rx: watch::Receiver<bool>,
}

pub(super) struct Channels {
    pub(super) command_tx: mpsc::Sender<Command>,
    pub(super) snapshot_rx: watch::Receiver<Conversations>,
    pub(super) event_tx: broadcast::Sender<Event>,
    pub(super) shutdown_tx: watch::Sender<bool>,
}

impl Actor {
    pub(in crate::messaging) fn new(
        store: Arc<dyn Store>,
        sessions: Arc<dyn SessionMessaging>,
        session_events: mpsc::Receiver<peer_session::Event>,
        snapshot: Conversations,
    ) -> (Self, Channels) {
        let (command_tx, commands) = mpsc::channel(COMMAND_CAPACITY);
        let (snapshot_tx, snapshot_rx) = watch::channel(snapshot.clone());
        let (event_tx, _) = broadcast::channel(EVENT_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let actor = Self {
            store,
            sessions,
            commands,
            session_events,
            session_events_open: true,
            snapshot,
            snapshot_tx,
            event_tx: event_tx.clone(),
            shutdown_rx,
        };
        let channels = Channels { command_tx, snapshot_rx, event_tx, shutdown_tx };
        (actor, channels)
    }

    pub(in crate::messaging) async fn run(mut self) -> Result<(), String> {
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
                                let _ = response_tx.send(Err(Error::CommandExpired));
                                continue;
                            }
                            let result = self.send_message(session_id, peer_id, body).await;
                            let fatal = fatal_error(&result);
                            let _ = response_tx.send(result);
                            if let Some(reason) = fatal {
                                return Err(reason);
                            }
                        }
                        Some(Command::Refresh { deadline, response_tx }) => {
                            if Instant::now() >= deadline {
                                let _ = response_tx.send(Err(Error::CommandExpired));
                                continue;
                            }
                            let result = self.refresh().await;
                            let fatal = fatal_error(&result);
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
                                if is_fatal_error(&error) {
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

    async fn shutdown(&mut self) -> Result<(), Error> {
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
                if is_fatal_error(&error) {
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
    ) -> Result<MessageId, Error> {
        let body = validate_body(body)?;
        self.validate_connected_session(session_id, &peer_id)?;

        let message_id = MessageId::new();
        let created_at = Utc::now();
        let pending = self
            .store
            .create_outgoing(session_id, message_id, peer_id.clone(), body.clone(), created_at)
            .await?;
        self.publish_record(pending)?;

        if let Err(error) =
            self.sessions.send_message(session_id, message_id, body, created_at).await
        {
            let status = if error == peer_session::Error::OutcomeUnknown {
                MessageStatus::Unknown
            } else {
                MessageStatus::Failed
            };
            let record = if status == MessageStatus::Unknown {
                self.store.mark_unknown(session_id, peer_id.clone(), message_id).await?
            } else {
                self.store.mark_failed(session_id, peer_id.clone(), message_id).await?
            }
            .ok_or(Error::MissingMessageTransition { message_id, status })?;
            self.publish_record(record)?;
            self.emit(Event::MessageStatusChanged { session_id, peer_id, message_id, status });
            return Err(error.into());
        }

        let sent =
            self.store.mark_sent(session_id, peer_id.clone(), message_id).await?.ok_or(
                Error::MissingMessageTransition { message_id, status: MessageStatus::Sent },
            )?;
        self.publish_record(sent)?;
        self.emit(Event::MessageStatusChanged {
            session_id,
            peer_id,
            message_id,
            status: MessageStatus::Sent,
        });
        Ok(message_id)
    }

    async fn refresh(&mut self) -> Result<(), Error> {
        self.snapshot = Self::load_snapshot(&self.store).await?;
        self.snapshot_tx.send_replace(self.snapshot.clone());
        Ok(())
    }

    fn validate_connected_session(
        &self,
        session_id: SessionId,
        expected_peer: &PeerId,
    ) -> Result<(), Error> {
        let sessions = self.sessions.snapshot();
        let session =
            sessions.session(session_id).ok_or(Error::SessionNotConnected { session_id })?;
        if &session.peer_id != expected_peer {
            return Err(Error::SessionPeerMismatch {
                session_id,
                expected_peer: expected_peer.clone(),
                actual_peer: session.peer_id.clone(),
            });
        }
        if session.phase != Phase::Connected {
            return Err(Error::SessionNotConnected { session_id });
        }
        Ok(())
    }

    async fn handle_session_event(&mut self, event: peer_session::Event) -> Result<(), Error> {
        match event {
            peer_session::Event::MessageReceived {
                session_id,
                peer_id,
                message_id,
                body,
                sent_at,
            } => {
                let body = validate_body(body)?;
                let received_at = Utc::now();
                let inserted = self
                    .store
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
                    self.publish_record(message)?;
                    self.emit(Event::IncomingMessage {
                        session_id,
                        peer_id: peer_id.clone(),
                        message_id,
                        body,
                    });
                }

                // Duplicate delivery is acknowledged again, but is never persisted twice.
                self.sessions.send_receipt(session_id, message_id, received_at).await?;
            }
            peer_session::Event::MessageReceiptReceived {
                session_id,
                peer_id,
                message_id,
                received_at,
            } => {
                if let Some(message) = self
                    .store
                    .mark_delivered(session_id, peer_id.clone(), message_id, received_at)
                    .await?
                {
                    self.publish_record(message)?;
                    self.emit(Event::MessageStatusChanged {
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

    fn publish_record(&mut self, record: MessageRecord) -> Result<(), Error> {
        let message = ConversationMessage::try_from(record)?;
        let peer_id = message.peer_id.clone();
        self.snapshot = with_upserted_message(&self.snapshot, message);
        self.snapshot_tx.send_replace(self.snapshot.clone());
        self.emit(Event::ConversationUpdated { peer_id });
        Ok(())
    }

    fn emit(&self, event: Event) {
        let _ = self.event_tx.send(event);
    }

    pub(in crate::messaging) async fn load_snapshot(
        store: &Arc<dyn Store>,
    ) -> Result<Conversations, Error> {
        let messages = store
            .list()
            .await?
            .into_iter()
            .map(ConversationMessage::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(build(messages))
    }
}

fn reject_stopping(command: Command) {
    match command {
        Command::SendMessage { response_tx, .. } => {
            let _ = response_tx.send(Err(Error::ServiceStopping));
        }
        Command::Refresh { response_tx, .. } => {
            let _ = response_tx.send(Err(Error::ServiceStopping));
        }
    }
}

fn fatal_error<T>(result: &Result<T, Error>) -> Option<String> {
    result.as_ref().err().filter(|error| is_fatal_error(error)).map(ToString::to_string)
}

fn is_fatal_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Store(_) | Error::MessageRecord(_) | Error::MissingMessageTransition { .. }
    )
}

fn validate_body(body: String) -> Result<String, Error> {
    let body = body.trim().to_owned();
    if body.is_empty() {
        return Err(Error::EmptyBody);
    }
    if body.len() > MAX_MESSAGE_BODY_BYTES {
        return Err(Error::MessageTooLarge { max: MAX_MESSAGE_BODY_BYTES });
    }
    Ok(body)
}

use std::{fmt, time::Instant};

use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::{Conversations, Error, Event, actor::Command};
use crate::{
    identity::PeerId,
    peer_session::{MessageId, SessionId},
};

#[derive(Clone)]
pub struct ServiceHandle {
    command_tx: mpsc::Sender<Command>,
    snapshot_rx: watch::Receiver<Conversations>,
    event_tx: broadcast::Sender<Event>,
    command_start_timeout: std::time::Duration,
}

impl ServiceHandle {
    pub(super) fn new(
        command_tx: mpsc::Sender<Command>,
        snapshot_rx: watch::Receiver<Conversations>,
        event_tx: broadcast::Sender<Event>,
        command_start_timeout: std::time::Duration,
    ) -> Self {
        Self { command_tx, snapshot_rx, event_tx, command_start_timeout }
    }

    pub fn snapshot(&self) -> Conversations {
        self.snapshot_rx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Conversations> {
        self.snapshot_rx.clone()
    }

    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.event_tx.subscribe()
    }

    pub async fn send_message(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        body: String,
    ) -> Result<MessageId, Error> {
        let (response_tx, response_rx) = oneshot::channel();
        self.enqueue(Command::SendMessage {
            deadline: Instant::now() + self.command_start_timeout,
            session_id,
            peer_id,
            body,
            response_tx,
        })?;
        response_rx.await.map_err(|_| Error::ResponseDropped)?
    }

    pub async fn refresh(&self) -> Result<(), Error> {
        let (response_tx, response_rx) = oneshot::channel();
        self.enqueue(Command::Refresh {
            deadline: Instant::now() + self.command_start_timeout,
            response_tx,
        })?;
        response_rx.await.map_err(|_| Error::ResponseDropped)?
    }

    pub(super) fn enqueue(&self, command: Command) -> Result<(), Error> {
        self.command_tx.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => Error::ServiceBusy,
            mpsc::error::TrySendError::Closed(_) => Error::ServiceStopped,
        })
    }
}

impl fmt::Debug for ServiceHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceHandle").finish_non_exhaustive()
    }
}

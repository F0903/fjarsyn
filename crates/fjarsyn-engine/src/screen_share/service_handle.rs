use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::{Command, Config, Error, Event, Selection, Shares};
use crate::{media::capture::PlatformItem, peer_session::SessionId};

/// Cloneable interface to the hosted screen-sharing capability.
#[derive(Clone)]
pub struct ServiceHandle {
    command_tx: mpsc::Sender<Command>,
    config_tx: watch::Sender<Config>,
    snapshot_rx: watch::Receiver<Shares>,
    event_tx: broadcast::Sender<Event>,
}

impl ServiceHandle {
    pub(super) fn new(
        command_tx: mpsc::Sender<Command>,
        config_tx: watch::Sender<Config>,
        snapshot_rx: watch::Receiver<Shares>,
        event_tx: broadcast::Sender<Event>,
    ) -> Self {
        Self { command_tx, config_tx, snapshot_rx, event_tx }
    }

    pub fn snapshot(&self) -> Shares {
        self.snapshot_rx.borrow().clone()
    }

    /// Subscribes to durable screen-share state and latest-frame updates.
    pub fn subscribe(&self) -> watch::Receiver<Shares> {
        self.snapshot_rx.clone()
    }

    /// Subscribes to transient pipeline failures and codec quarantine events.
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.event_tx.subscribe()
    }

    /// Reserves the single local screen-share slot for capture-source selection.
    pub async fn begin_selection(&self, session_id: SessionId) -> Result<Selection, Error> {
        let selection = Selection::new(session_id);
        let mut cancellation = SelectionCancellation::new(selection.clone());
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(Command::BeginSelection { selection: selection.clone(), reply: reply_tx })
            .await
            .map_err(|_| Error::ServiceUnavailable)?;
        reply_rx.await.map_err(|_| Error::ResponseDropped)?.map_err(Error::Operation)?;
        cancellation.disarm();
        Ok(selection)
    }

    /// Cancels exactly the selection represented by `selection`.
    pub async fn cancel_selection(&self, selection: Selection) -> Result<(), Error> {
        let mut cancellation = SelectionCancellation::new(selection.clone());
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(Command::CancelSelection { selection, reply: reply_tx })
            .await
            .map_err(|_| Error::ServiceUnavailable)?;
        let result = reply_rx.await.map_err(|_| Error::ResponseDropped)?.map_err(Error::Operation);
        cancellation.disarm();
        result
    }

    /// Fails exactly the selection represented by `selection`.
    pub async fn selection_failed(
        &self,
        selection: Selection,
        reason: String,
    ) -> Result<(), Error> {
        let mut cancellation = SelectionCancellation::new(selection.clone());
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(Command::FailSelection { selection, reason, reply: reply_tx })
            .await
            .map_err(|_| Error::ServiceUnavailable)?;
        let result = reply_rx.await.map_err(|_| Error::ResponseDropped)?.map_err(Error::Operation);
        cancellation.disarm();
        result
    }

    /// Starts the reserved authenticated share, capture, encoder, and transport transaction.
    ///
    /// The transaction is owned by the hosted service. Dropping this future
    /// marks its exact reservation identity cancelled; the service then
    /// rolls back any accepted peer-session mutation and local pipeline.
    pub async fn start_screen_share(
        &self,
        selection: Selection,
        item: PlatformItem,
    ) -> Result<(), Error> {
        let mut cancellation = SelectionCancellation::new(selection.clone());
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(Command::StartScreenShare { selection, item, reply: reply_tx })
            .await
            .map_err(|_| Error::ServiceUnavailable)?;
        let result = reply_rx.await.map_err(|_| Error::ResponseDropped)?.map_err(Error::Operation);
        if result.is_ok() {
            cancellation.commit();
        }
        cancellation.disarm();
        result
    }

    /// Stops local media and reconciles the exact authenticated share to inactive.
    pub async fn stop_screen_share(&self, session_id: SessionId) -> Result<(), Error> {
        self.dispatch(|reply| Command::StopScreenShare { session_id, reply }).await
    }

    pub fn update_config(&self, config: Config) {
        self.config_tx.send_replace(config);
    }

    async fn dispatch<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, String>>) -> Command,
    ) -> Result<T, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx.send(build(reply_tx)).await.map_err(|_| Error::ServiceUnavailable)?;
        reply_rx.await.map_err(|_| Error::ResponseDropped)?.map_err(Error::Operation)
    }
}

struct SelectionCancellation {
    selection: Selection,
    armed: bool,
}

impl SelectionCancellation {
    fn new(selection: Selection) -> Self {
        Self { selection, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn commit(&self) {
        self.selection.commit();
    }
}

impl Drop for SelectionCancellation {
    fn drop(&mut self) {
        if self.armed {
            self.selection.cancel();
        }
    }
}

impl std::fmt::Debug for ServiceHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ServiceHandle").finish_non_exhaustive()
    }
}

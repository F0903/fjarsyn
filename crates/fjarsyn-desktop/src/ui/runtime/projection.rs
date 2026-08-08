//! Read-only service-to-UI projection workers.

use fjarsyn_engine::{messaging, peer_session, presence, screen_share};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::ui::runtime::{Event, event::ScreenShareUpdateSlot};

/// Owns every long-lived task that projects service state into the UI.
pub(in crate::ui::runtime) struct Workers {
    tasks: Vec<JoinHandle<Result<(), String>>>,
}

impl Workers {
    pub(in crate::ui::runtime) fn start(
        presence: &presence::ServiceHandle,
        sessions: &peer_session::ServiceHandle,
        messaging: &messaging::ServiceHandle,
        screen_share: &screen_share::ServiceHandle,
        event_tx: mpsc::Sender<Event>,
    ) -> Self {
        let mut tasks = vec![spawn_presence_projection(presence, event_tx.clone())];
        tasks.extend(spawn_session_projection(sessions, event_tx.clone()));
        tasks.extend(spawn_messaging_projection(messaging, event_tx.clone()));
        tasks.extend(spawn_screen_share_projection(screen_share, event_tx));
        Self { tasks }
    }

    pub(in crate::ui::runtime) async fn shutdown_until(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> bool {
        let tasks = self.tasks.drain(..).collect::<Vec<_>>();
        for task in &tasks {
            task.abort();
        }
        let Ok(results) = tokio::time::timeout_at(deadline, futures::future::join_all(tasks)).await
        else {
            return false;
        };
        let mut clean = true;
        for result in results {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%error, "projection worker stopped unexpectedly");
                    clean = false;
                }
                Err(error) if error.is_cancelled() => {}
                Err(error) => {
                    tracing::warn!(%error, "projection worker failed");
                    clean = false;
                }
            }
        }
        clean
    }

    pub(in crate::ui::runtime) fn abort(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.abort();
    }
}

fn spawn_session_projection(
    sessions: &peer_session::ServiceHandle,
    event_tx: mpsc::Sender<Event>,
) -> [JoinHandle<Result<(), String>>; 2] {
    let mut snapshots = sessions.subscribe();
    let snapshot_tx = event_tx.clone();
    let snapshot_worker = tokio::spawn(async move {
        loop {
            snapshots
                .changed()
                .await
                .map_err(|_| "peer-session snapshot source closed".to_owned())?;
            let snapshot = snapshots.borrow_and_update().clone();
            snapshot_tx
                .send(Event::Sessions(snapshot))
                .await
                .map_err(|_| "UI runtime event channel closed".to_owned())?;
        }
    });

    let mut events = sessions.events();
    let event_worker = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if event_tx.send(Event::SessionChange(event)).await.is_err() {
                        return Err("UI runtime event channel closed".into());
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("peer-session event projection lagged by {skipped} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err("peer-session event source closed".into());
                }
            }
        }
    });

    [snapshot_worker, event_worker]
}

fn spawn_presence_projection(
    presence: &presence::ServiceHandle,
    event_tx: mpsc::Sender<Event>,
) -> JoinHandle<Result<(), String>> {
    let mut snapshots = presence.subscribe();

    tokio::spawn(async move {
        loop {
            snapshots.changed().await.map_err(|_| "presence snapshot source closed".to_owned())?;
            let snapshot = snapshots.borrow_and_update().clone();
            if event_tx.send(Event::Presence(snapshot)).await.is_err() {
                return Err("UI runtime event channel closed".into());
            }
        }
    })
}

fn spawn_messaging_projection(
    messaging: &messaging::ServiceHandle,
    event_tx: mpsc::Sender<Event>,
) -> [JoinHandle<Result<(), String>>; 2] {
    let mut snapshots = messaging.subscribe();
    let snapshot_tx = event_tx.clone();
    let snapshot_worker = tokio::spawn(async move {
        loop {
            snapshots.changed().await.map_err(|_| "messaging snapshot source closed".to_owned())?;
            let snapshot = snapshots.borrow_and_update().clone();
            snapshot_tx
                .send(Event::Messaging(snapshot))
                .await
                .map_err(|_| "UI runtime event channel closed".to_owned())?;
        }
    });

    let mut events = messaging.events();
    let event_worker = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if event_tx.send(Event::MessagingChange(event)).await.is_err() {
                        return Err("UI runtime event channel closed".into());
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("messaging event projection lagged by {skipped} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err("messaging event source closed".into());
                }
            }
        }
    });

    [snapshot_worker, event_worker]
}

fn spawn_screen_share_projection(
    screen_share: &screen_share::ServiceHandle,
    event_tx: mpsc::Sender<Event>,
) -> [JoinHandle<Result<(), String>>; 2] {
    let mut snapshots = screen_share.subscribe();
    let updates = ScreenShareUpdateSlot::default();
    let initial = snapshots.borrow_and_update().clone();
    let initial_update = updates
        .replace(initial)
        .expect("a new screen-share update slot schedules its initial notification");
    let snapshot_tx = event_tx.clone();
    let snapshot_worker = tokio::spawn(async move {
        snapshot_tx
            .send(Event::ScreenShareSnapshotReady(initial_update))
            .await
            .map_err(|_| "UI runtime event channel closed".to_owned())?;
        loop {
            snapshots
                .changed()
                .await
                .map_err(|_| "screen-share snapshot source closed".to_owned())?;
            let snapshot = snapshots.borrow_and_update().clone();
            if let Some(update) = updates.replace(snapshot) {
                snapshot_tx
                    .send(Event::ScreenShareSnapshotReady(update))
                    .await
                    .map_err(|_| "UI runtime event channel closed".to_owned())?;
            }
        }
    });

    let mut events = screen_share.events();
    let event_worker = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if event_tx.send(Event::ScreenShareChange(event)).await.is_err() {
                        return Err("UI runtime event channel closed".into());
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("screen-share event projection lagged by {skipped} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err("screen-share event source closed".into());
                }
            }
        }
    });

    [snapshot_worker, event_worker]
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Workers;

    #[tokio::test]
    async fn worker_panic_is_not_reported_as_clean_shutdown() {
        let task = tokio::spawn(async { panic!("projection failed") });
        tokio::task::yield_now().await;
        let mut workers = Workers { tasks: vec![task] };

        let clean =
            workers.shutdown_until(tokio::time::Instant::now() + Duration::from_secs(1)).await;

        assert!(!clean);
    }

    #[tokio::test]
    async fn intentional_worker_cancellation_is_a_clean_shutdown() {
        let task = tokio::spawn(std::future::pending::<Result<(), String>>());
        let mut workers = Workers { tasks: vec![task] };

        let clean =
            workers.shutdown_until(tokio::time::Instant::now() + Duration::from_secs(1)).await;

        assert!(clean);
    }

    #[tokio::test]
    async fn unexpected_worker_exit_is_not_reported_as_clean_shutdown() {
        let task = tokio::spawn(async { Err("source closed".to_owned()) });
        tokio::task::yield_now().await;
        let mut workers = Workers { tasks: vec![task] };

        let clean =
            workers.shutdown_until(tokio::time::Instant::now() + Duration::from_secs(1)).await;

        assert!(!clean);
    }
}

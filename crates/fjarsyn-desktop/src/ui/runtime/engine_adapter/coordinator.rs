use fjarsyn_engine::{messaging, peer_session, presence, screen_share};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use super::{
    engine_state::{EngineState, Publisher},
    failure::{Failure, Source},
    notice::{Notice, Publisher as NoticePublisher},
};

pub(super) enum Exit {
    Shutdown,
    #[cfg(test)]
    Unexpected,
}

/// Owns and polls every adapted engine source in one failure domain.
pub(super) struct Coordinator {
    presence: watch::Receiver<presence::NearbyPeers>,
    sessions: watch::Receiver<peer_session::Sessions>,
    session_events: broadcast::Receiver<peer_session::Event>,
    messaging: watch::Receiver<messaging::Conversations>,
    messaging_events: broadcast::Receiver<messaging::Event>,
    screen_share: watch::Receiver<screen_share::Shares>,
    screen_share_events: broadcast::Receiver<screen_share::Event>,
    state: Publisher,
    notices: NoticePublisher,
}

impl Coordinator {
    pub(super) fn prepare(
        presence_handle: &presence::ServiceHandle,
        session_handle: &peer_session::ServiceHandle,
        messaging_handle: &messaging::ServiceHandle,
        screen_share_handle: &screen_share::ServiceHandle,
        notice_tx: mpsc::Sender<Notice>,
    ) -> (Self, watch::Receiver<EngineState>) {
        let mut presence_rx = presence_handle.subscribe();
        let mut sessions_rx = session_handle.subscribe();
        let session_events_rx = session_handle.events();
        let mut messaging_rx = messaging_handle.subscribe();
        let messaging_events_rx = messaging_handle.events();
        let mut screen_share_rx = screen_share_handle.subscribe();
        let screen_share_events_rx = screen_share_handle.events();
        let initial = EngineState {
            presence: presence_rx.borrow_and_update().clone(),
            sessions: sessions_rx.borrow_and_update().clone(),
            messaging: messaging_rx.borrow_and_update().clone(),
            screen_share: screen_share_rx.borrow_and_update().clone(),
        };
        let (state, state_rx) = Publisher::new(initial);
        (
            Self {
                presence: presence_rx,
                sessions: sessions_rx,
                session_events: session_events_rx,
                messaging: messaging_rx,
                messaging_events: messaging_events_rx,
                screen_share: screen_share_rx,
                screen_share_events: screen_share_events_rx,
                state,
                notices: NoticePublisher::new(notice_tx),
            },
            state_rx,
        )
    }

    pub(super) async fn run(
        mut self,
        mut shutdown: oneshot::Receiver<()>,
    ) -> Result<Exit, Failure> {
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown => return Ok(Exit::Shutdown),
                result = self.receive_next() => result?,
            }
        }
    }

    /// Polls engine sources fairly while the outer loop gives shutdown priority.
    async fn receive_next(&mut self) -> Result<(), Failure> {
        tokio::select! {
            result = self.presence.changed() => {
                require_source(result, Source::PresenceState)?;
                self.state.presence(self.presence.borrow_and_update().clone())?;
            }
            result = self.sessions.changed() => {
                require_source(result, Source::SessionState)?;
                self.state.sessions(self.sessions.borrow_and_update().clone())?;
            }
            result = self.messaging.changed() => {
                require_source(result, Source::MessagingState)?;
                self.state.messaging(self.messaging.borrow_and_update().clone())?;
            }
            result = self.screen_share.changed() => {
                require_source(result, Source::ScreenShareState)?;
                self.state.screen_share(self.screen_share.borrow_and_update().clone())?;
            }
            result = self.session_events.recv() => {
                if let Some(notice) = receive_event(result, Source::SessionEvents, Notice::from_session)? {
                    self.notices.publish(notice, Source::SessionEvents)?;
                }
            }
            result = self.messaging_events.recv() => {
                if let Some(notice) = receive_event(result, Source::MessagingEvents, Notice::from_messaging)? {
                    self.notices.publish(notice, Source::MessagingEvents)?;
                }
            }
            result = self.screen_share_events.recv() => {
                if let Some(notice) = receive_event(result, Source::ScreenShareEvents, Notice::from_screen_share)? {
                    self.notices.publish(notice, Source::ScreenShareEvents)?;
                }
            }
        }
        Ok(())
    }
}

fn require_source(
    result: Result<(), watch::error::RecvError>,
    source: Source,
) -> Result<(), Failure> {
    result.map_err(|_| Failure::source_closed(source))
}

fn receive_event<T>(
    result: Result<T, broadcast::error::RecvError>,
    source: Source,
    map: impl FnOnce(T) -> Option<Notice>,
) -> Result<Option<Notice>, Failure> {
    match result {
        Ok(event) => Ok(map(event)),
        Err(broadcast::error::RecvError::Lagged(skipped)) => {
            tracing::warn!(%source, skipped, "transient desktop notices lagged");
            Ok(None)
        }
        Err(broadcast::error::RecvError::Closed) => Err(Failure::source_closed(source)),
    }
}

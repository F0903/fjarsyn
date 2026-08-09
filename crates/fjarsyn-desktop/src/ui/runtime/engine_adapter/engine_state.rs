use fjarsyn_engine::{messaging, peer_session, presence, screen_share};
use tokio::sync::watch;

use super::failure::{Failure, Source};

/// Latest desktop-visible aggregate of live engine capability state.
///
/// Its fields come from independent capability watch channels, so this is
/// neither exhaustive engine state nor an atomic cross-capability snapshot.
#[derive(Debug, Clone, Default)]
pub(in crate::ui) struct EngineState {
    pub(in crate::ui) presence: presence::NearbyPeers,
    pub(in crate::ui) sessions: peer_session::Sessions,
    pub(in crate::ui) messaging: messaging::Conversations,
    pub(in crate::ui) screen_share: screen_share::Shares,
}

/// Publishes the current desktop aggregate through a retained watch channel.
pub(super) struct Publisher {
    current: EngineState,
    state_tx: watch::Sender<EngineState>,
}

impl Publisher {
    pub(super) fn new(current: EngineState) -> (Self, watch::Receiver<EngineState>) {
        let (state_tx, state_rx) = watch::channel(current.clone());
        (Self { current, state_tx }, state_rx)
    }

    pub(super) fn presence(&mut self, nearby_peers: presence::NearbyPeers) -> Result<(), Failure> {
        self.current.presence = nearby_peers;
        self.publish(Source::PresenceState)
    }

    pub(super) fn sessions(&mut self, sessions: peer_session::Sessions) -> Result<(), Failure> {
        self.current.sessions = sessions;
        self.publish(Source::SessionState)
    }

    pub(super) fn messaging(
        &mut self,
        conversations: messaging::Conversations,
    ) -> Result<(), Failure> {
        self.current.messaging = conversations;
        self.publish(Source::MessagingState)
    }

    pub(super) fn screen_share(&mut self, shares: screen_share::Shares) -> Result<(), Failure> {
        self.current.screen_share = shares;
        self.publish(Source::ScreenShareState)
    }

    fn publish(&self, source: Source) -> Result<(), Failure> {
        self.state_tx
            .send(self.current.clone())
            .map_err(|_| Failure::runtime_channel_closed(source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_channel_retains_the_latest_complete_engine_state() {
        let (mut publisher, mut state_rx) = Publisher::new(EngineState::default());

        publisher.presence(presence::NearbyPeers::default()).unwrap();
        publisher.sessions(peer_session::Sessions::default()).unwrap();
        publisher.messaging(messaging::Conversations::default()).unwrap();

        assert!(state_rx.has_changed().unwrap());
        let latest = state_rx.borrow_and_update().clone();
        assert_eq!(latest.presence, presence::NearbyPeers::default());
        assert_eq!(latest.sessions, peer_session::Sessions::default());
        assert_eq!(latest.messaging, messaging::Conversations::default());
        assert!(!state_rx.has_changed().unwrap());

        publisher.screen_share(screen_share::Shares::default()).unwrap();
        assert!(state_rx.has_changed().unwrap());
    }

    #[test]
    fn publishing_fails_after_every_runtime_receiver_is_dropped() {
        let (mut publisher, state_rx) = Publisher::new(EngineState::default());
        drop(state_rx);

        let failure = publisher.presence(presence::NearbyPeers::default()).unwrap_err();

        assert_eq!(failure.source(), Source::PresenceState);
    }
}

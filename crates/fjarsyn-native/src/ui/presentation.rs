use fjarsyn_core::peer_session::PeerSessionPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresencePresentation {
    Away,
    Nearby,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPresentation {
    Disconnected,
    Requesting,
    Incoming,
    Negotiating,
    Connected,
    Disconnecting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerPresentation {
    pub presence: PresencePresentation,
    pub session: SessionPresentation,
}

impl PeerPresentation {
    pub fn can_connect(self) -> bool {
        self.presence == PresencePresentation::Nearby
            && self.session == SessionPresentation::Disconnected
    }

    pub fn can_accept_or_reject(self) -> bool {
        self.session == SessionPresentation::Incoming
    }

    pub fn can_disconnect(self) -> bool {
        matches!(
            self.session,
            SessionPresentation::Requesting
                | SessionPresentation::Negotiating
                | SessionPresentation::Connected
        )
    }

    pub fn capabilities_ready(self) -> bool {
        self.session == SessionPresentation::Connected
    }

    /// Deleting a contact or replacing its trusted key is a trust revocation.
    /// It is only safe once no live or pending session owns that identity.
    pub fn can_mutate_trust(self) -> bool {
        self.session == SessionPresentation::Disconnected
    }
}

pub fn project_peer(nearby: bool, phase: Option<PeerSessionPhase>) -> PeerPresentation {
    PeerPresentation {
        presence: if nearby { PresencePresentation::Nearby } else { PresencePresentation::Away },
        session: match phase {
            None => SessionPresentation::Disconnected,
            Some(PeerSessionPhase::Requesting) => SessionPresentation::Requesting,
            Some(PeerSessionPhase::Incoming) => SessionPresentation::Incoming,
            Some(PeerSessionPhase::Negotiating) => SessionPresentation::Negotiating,
            Some(PeerSessionPhase::Connected) => SessionPresentation::Connected,
            Some(PeerSessionPhase::Disconnecting) => SessionPresentation::Disconnecting,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHASES: [Option<PeerSessionPhase>; 6] = [
        None,
        Some(PeerSessionPhase::Requesting),
        Some(PeerSessionPhase::Incoming),
        Some(PeerSessionPhase::Negotiating),
        Some(PeerSessionPhase::Connected),
        Some(PeerSessionPhase::Disconnecting),
    ];

    #[test]
    fn projects_every_presence_and_session_combination_independently() {
        for nearby in [false, true] {
            for phase in PHASES {
                let projection = project_peer(nearby, phase);
                assert_eq!(
                    projection.presence,
                    if nearby { PresencePresentation::Nearby } else { PresencePresentation::Away }
                );
                assert_eq!(
                    projection.session,
                    match phase {
                        None => SessionPresentation::Disconnected,
                        Some(PeerSessionPhase::Requesting) => SessionPresentation::Requesting,
                        Some(PeerSessionPhase::Incoming) => SessionPresentation::Incoming,
                        Some(PeerSessionPhase::Negotiating) => SessionPresentation::Negotiating,
                        Some(PeerSessionPhase::Connected) => SessionPresentation::Connected,
                        Some(PeerSessionPhase::Disconnecting) => SessionPresentation::Disconnecting,
                    }
                );
            }
        }
    }

    #[test]
    fn presence_loss_never_changes_a_live_session_projection() {
        for phase in PHASES.into_iter().flatten() {
            assert_eq!(
                project_peer(false, Some(phase)).session,
                project_peer(true, Some(phase)).session
            );
        }
    }

    #[test]
    fn actions_are_gated_by_deliberate_session_state() {
        assert!(!project_peer(false, None).can_connect());
        assert!(project_peer(true, None).can_connect());
        assert!(project_peer(false, Some(PeerSessionPhase::Incoming)).can_accept_or_reject());
        assert!(project_peer(false, Some(PeerSessionPhase::Requesting)).can_disconnect());
        assert!(project_peer(false, Some(PeerSessionPhase::Negotiating)).can_disconnect());
        assert!(project_peer(false, Some(PeerSessionPhase::Connected)).can_disconnect());
        assert!(!project_peer(false, Some(PeerSessionPhase::Disconnecting)).can_disconnect());
        assert!(project_peer(false, Some(PeerSessionPhase::Connected)).capabilities_ready());
        assert!(!project_peer(true, Some(PeerSessionPhase::Negotiating)).capabilities_ready());
        assert!(project_peer(false, None).can_mutate_trust());
        for phase in PHASES.into_iter().flatten() {
            assert!(!project_peer(false, Some(phase)).can_mutate_trust());
        }
    }
}

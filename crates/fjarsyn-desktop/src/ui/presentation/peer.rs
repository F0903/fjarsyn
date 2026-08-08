use fjarsyn_engine::peer_session::Phase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Presence {
    Away,
    Nearby,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Session {
    Disconnected,
    Requesting,
    Incoming,
    Negotiating,
    Connected,
    Reconnecting,
    Disconnecting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) struct Peer {
    pub(super) presence: Presence,
    pub(super) session: Session,
}

impl Peer {
    pub(in crate::ui) fn can_connect(self) -> bool {
        self.presence == Presence::Nearby && self.session == Session::Disconnected
    }

    pub(in crate::ui) fn can_disconnect(self) -> bool {
        matches!(
            self.session,
            Session::Requesting | Session::Negotiating | Session::Connected | Session::Reconnecting
        )
    }

    pub(in crate::ui) fn capabilities_ready(self) -> bool {
        self.session == Session::Connected
    }

    /// Deleting a contact or replacing its trusted key is a trust revocation.
    /// It is only safe once no live or pending session owns that identity.
    pub(in crate::ui) fn can_mutate_trust(self) -> bool {
        self.session == Session::Disconnected
    }
}

pub(in crate::ui) fn project_peer(nearby: bool, phase: Option<Phase>) -> Peer {
    Peer {
        presence: if nearby { Presence::Nearby } else { Presence::Away },
        session: match phase {
            None => Session::Disconnected,
            Some(Phase::Requesting) => Session::Requesting,
            Some(Phase::Incoming) => Session::Incoming,
            Some(Phase::Negotiating) => Session::Negotiating,
            Some(Phase::Connected) => Session::Connected,
            Some(Phase::Reconnecting) => Session::Reconnecting,
            Some(Phase::Disconnecting) => Session::Disconnecting,
        },
    }
}

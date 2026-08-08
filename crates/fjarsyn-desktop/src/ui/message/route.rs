use fjarsyn_engine::identity::PeerId;

/// Stable, contact-oriented application routes.
///
/// A peer route identifies presentation only. Entering or leaving it never
/// establishes or tears down a network session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::ui) enum Route {
    Home,
    Contacts,
    Peer { peer_id: PeerId },
    Settings,
}

impl Route {
    pub(in crate::ui) fn same_screen(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Home, Self::Home)
            | (Self::Contacts, Self::Contacts)
            | (Self::Settings, Self::Settings) => true,
            (Self::Peer { peer_id: left }, Self::Peer { peer_id: right }) => left == right,
            _ => false,
        }
    }
}

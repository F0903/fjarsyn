use tokio::time::Instant;

use crate::peer_session::TransportGeneration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::peer_session) struct Attempt {
    generation: TransportGeneration,
    deadline: Instant,
    engaged: bool,
    authorized: bool,
}

impl Attempt {
    pub(super) fn new(generation: TransportGeneration, deadline: Instant) -> Self {
        Self { generation, deadline, engaged: false, authorized: false }
    }

    pub(super) fn mark_engaged(&mut self) {
        self.engaged = true;
    }

    pub(super) fn mark_authorized(&mut self) {
        self.authorized = true;
    }

    pub(in crate::peer_session) fn generation(self) -> TransportGeneration {
        self.generation
    }

    pub(in crate::peer_session) fn deadline(self) -> Instant {
        self.deadline
    }

    pub(in crate::peer_session) fn authorized(self) -> bool {
        self.authorized
    }

    pub(in crate::peer_session) fn engaged(self) -> bool {
        self.engaged
    }
}

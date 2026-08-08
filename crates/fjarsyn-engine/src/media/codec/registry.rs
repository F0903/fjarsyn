//! Codec worker reservations and registry-owned lifecycle directives.

use std::sync::{Arc, atomic::AtomicBool};

use tokio::sync::watch;

use crate::media::codec::Poison;

pub(in crate::media::codec) type WorkerId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::media::codec) enum WorkerDirective {
    Run,
    Stop,
    Poisoned(Poison),
    ServiceShutdown,
}

pub(in crate::media::codec) struct WorkerReservationParts {
    pub(in crate::media::codec) id: WorkerId,
    pub(in crate::media::codec) directive: watch::Receiver<WorkerDirective>,
    pub(in crate::media::codec) accepting: Arc<AtomicBool>,
    pub(in crate::media::codec) publishing: Arc<AtomicBool>,
}

pub(in crate::media::codec) struct WorkerReservation {
    id: WorkerId,
    directive: watch::Receiver<WorkerDirective>,
    accepting: Arc<AtomicBool>,
    publishing: Arc<AtomicBool>,
}

impl WorkerReservation {
    pub(in crate::media::codec) fn new(
        id: WorkerId,
        directive: watch::Receiver<WorkerDirective>,
        accepting: Arc<AtomicBool>,
        publishing: Arc<AtomicBool>,
    ) -> Self {
        Self { id, directive, accepting, publishing }
    }

    pub(in crate::media::codec) fn into_parts(self) -> WorkerReservationParts {
        WorkerReservationParts {
            id: self.id,
            directive: self.directive,
            accepting: self.accepting,
            publishing: self.publishing,
        }
    }
}

//! Registry reservation granted before a native codec thread is spawned.

use std::sync::{Arc, atomic::AtomicBool};

use tokio::sync::watch;

use super::WorkerDirective;

pub(in crate::services::codec_service) type WorkerId = u64;

pub(in crate::services::codec_service) struct WorkerReservation {
    id: WorkerId,
    directive: watch::Receiver<WorkerDirective>,
    accepting: Arc<AtomicBool>,
    publishing: Arc<AtomicBool>,
}

pub(in crate::services::codec_service) struct WorkerReservationParts {
    pub(in crate::services::codec_service) id: WorkerId,
    pub(in crate::services::codec_service) directive: watch::Receiver<WorkerDirective>,
    pub(in crate::services::codec_service) accepting: Arc<AtomicBool>,
    pub(in crate::services::codec_service) publishing: Arc<AtomicBool>,
}

impl WorkerReservation {
    pub(in crate::services::codec_service) fn new(
        id: WorkerId,
        directive: watch::Receiver<WorkerDirective>,
        accepting: Arc<AtomicBool>,
        publishing: Arc<AtomicBool>,
    ) -> Self {
        Self { id, directive, accepting, publishing }
    }

    pub(in crate::services::codec_service) fn into_parts(self) -> WorkerReservationParts {
        WorkerReservationParts {
            id: self.id,
            directive: self.directive,
            accepting: self.accepting,
            publishing: self.publishing,
        }
    }
}

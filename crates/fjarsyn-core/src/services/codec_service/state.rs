//! Shared codec-service registry, health, and shutdown state.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::sync::watch;

use super::{
    CodecDirection, CodecDirectionState, CodecOperation, CodecPoison, CodecPoisonReason, Config,
    Error, Snapshot,
    backend::CodecBackendFactory,
    registry::{WorkerDirective, WorkerId, WorkerReservation},
};

struct WorkerRecord {
    direction: CodecDirection,
    directive: watch::Sender<WorkerDirective>,
    accepting: Arc<AtomicBool>,
    publishing: Arc<AtomicBool>,
}

struct StateData {
    shutting_down: bool,
    next_worker_id: WorkerId,
    snapshot: Snapshot,
    workers: BTreeMap<WorkerId, WorkerRecord>,
    quarantined_workers: usize,
}

pub(in crate::services::codec_service) struct State {
    config: Config,
    data: Mutex<StateData>,
    snapshot_tx: watch::Sender<Snapshot>,
    backend: Arc<dyn CodecBackendFactory>,
}

impl State {
    pub(in crate::services::codec_service) fn new(
        config: Config,
        backend: Arc<dyn CodecBackendFactory>,
    ) -> Arc<Self> {
        let snapshot = Snapshot::default();
        let (snapshot_tx, _) = watch::channel(snapshot.clone());
        Arc::new(Self {
            config,
            data: Mutex::new(StateData {
                shutting_down: false,
                next_worker_id: 1,
                snapshot,
                workers: BTreeMap::new(),
                quarantined_workers: 0,
            }),
            snapshot_tx,
            backend,
        })
    }

    pub(in crate::services::codec_service) fn call_timeout(&self) -> Duration {
        self.config.call_timeout
    }

    pub(in crate::services::codec_service) fn stop_timeout(&self) -> Duration {
        self.config.stop_timeout
    }

    pub(in crate::services::codec_service) fn backend(&self) -> Arc<dyn CodecBackendFactory> {
        self.backend.clone()
    }

    pub(in crate::services::codec_service) fn snapshot(&self) -> Snapshot {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshot.clone()
    }

    pub(in crate::services::codec_service) fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.snapshot_tx.subscribe()
    }

    pub(in crate::services::codec_service) fn reserve_worker(
        &self,
        direction: CodecDirection,
    ) -> Result<WorkerReservation, Error> {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if data.shutting_down {
            return Err(Error::ShuttingDown);
        }
        if let CodecDirectionState::RestartRequired(poison) = data.snapshot.direction(direction) {
            return Err(Error::RestartRequired(poison.clone()));
        }

        let id = data.next_worker_id;
        data.next_worker_id = data.next_worker_id.saturating_add(1);
        let (directive, receiver) = watch::channel(WorkerDirective::Run);
        let accepting = Arc::new(AtomicBool::new(true));
        let publishing = Arc::new(AtomicBool::new(true));
        data.workers.insert(
            id,
            WorkerRecord {
                direction,
                directive,
                accepting: accepting.clone(),
                publishing: publishing.clone(),
            },
        );
        Ok(WorkerReservation::new(id, receiver, accepting, publishing))
    }

    pub(in crate::services::codec_service) fn remove_worker(&self, id: WorkerId) {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).workers.remove(&id);
    }

    pub(in crate::services::codec_service) fn quarantine_worker(&self, id: WorkerId) -> bool {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if data.workers.remove(&id).is_none() {
            return false;
        }
        data.quarantined_workers = data.quarantined_workers.saturating_add(1);
        true
    }

    pub(in crate::services::codec_service) fn release_quarantined_worker(&self) {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        data.quarantined_workers = data.quarantined_workers.saturating_sub(1);
    }

    pub(in crate::services::codec_service) fn request_stop(&self, id: WorkerId) {
        let data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(worker) = data.workers.get(&id)
            && matches!(*worker.directive.borrow(), WorkerDirective::Run)
        {
            worker.accepting.store(false, Ordering::Release);
            worker.publishing.store(false, Ordering::Release);
            worker.directive.send_replace(WorkerDirective::Stop);
        }
    }

    pub(in crate::services::codec_service) fn poison(
        &self,
        direction: CodecDirection,
        operation: CodecOperation,
        reason: CodecPoisonReason,
    ) -> CodecPoison {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let existing = match data.snapshot.direction(direction) {
            CodecDirectionState::Available => None,
            CodecDirectionState::RestartRequired(poison) => Some(poison.clone()),
        };
        if let Some(existing) = existing {
            return existing;
        }

        let poison = CodecPoison { direction, operation, reason };
        match direction {
            CodecDirection::Encode => {
                data.snapshot.encode = CodecDirectionState::RestartRequired(poison.clone());
            }
            CodecDirection::Decode => {
                data.snapshot.decode = CodecDirectionState::RestartRequired(poison.clone());
            }
        }
        for worker in data.workers.values().filter(|worker| worker.direction == direction) {
            worker.accepting.store(false, Ordering::Release);
            worker.publishing.store(false, Ordering::Release);
            worker.directive.send_replace(WorkerDirective::Poisoned(poison.clone()));
        }
        self.snapshot_tx.send_replace(data.snapshot.clone());
        poison
    }

    pub(in crate::services::codec_service) fn begin_shutdown(&self) {
        let mut data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if data.shutting_down {
            return;
        }
        data.shutting_down = true;
        for worker in data.workers.values() {
            worker.accepting.store(false, Ordering::Release);
            worker.publishing.store(false, Ordering::Release);
            if !matches!(*worker.directive.borrow(), WorkerDirective::Poisoned(_)) {
                worker.directive.send_replace(WorkerDirective::ServiceShutdown);
            }
        }
    }

    pub(in crate::services::codec_service) fn poison_unfinished_shutdowns(&self) {
        let directions = {
            let data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            data.workers.values().map(|worker| worker.direction).collect::<Vec<_>>()
        };
        for direction in directions {
            self.poison(direction, CodecOperation::Shutdown, CodecPoisonReason::DeadlineExceeded);
        }
    }

    pub(in crate::services::codec_service) fn shutdown_counts(&self) -> (usize, usize) {
        let data = self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        (data.workers.len(), data.quarantined_workers)
    }

    pub(in crate::services::codec_service) fn detach_unfinished_workers(&self) {
        self.data.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).workers.clear();
    }
}

use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use super::PeerSessionError;

/// Monotonically identifies the ICE credentials and callbacks belonging to one
/// negotiated transport. Generation zero is the initial connection; a restart
/// may only propose the exact next generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TransportGeneration(u64);

impl TransportGeneration {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) const fn from_value(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    pub(crate) fn next(self) -> Result<Self, PeerSessionError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| PeerSessionError::Protocol("transport generation exhausted".into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IceRestartAttempt {
    generation: TransportGeneration,
    deadline: Instant,
    engaged: bool,
    authorized: bool,
}

/// Pure restart-attempt ownership and generation fencing. Network and WebRTC
/// side effects remain actor-owned; this value only admits valid transitions.
#[derive(Debug)]
pub(crate) struct IceRestartCoordinator {
    committed: TransportGeneration,
    attempt: Option<IceRestartAttempt>,
}

impl Default for IceRestartCoordinator {
    fn default() -> Self {
        Self { committed: TransportGeneration::INITIAL, attempt: None }
    }
}

impl IceRestartCoordinator {
    pub(crate) fn committed(&self) -> TransportGeneration {
        self.committed
    }

    pub(crate) fn active(&self) -> Option<IceRestartAttempt> {
        self.attempt
    }

    pub(crate) fn begin_local(
        &mut self,
        deadline: Instant,
    ) -> Result<TransportGeneration, PeerSessionError> {
        if self.attempt.is_some() {
            return Err(PeerSessionError::Protocol("ICE restart is already in progress".into()));
        }
        let generation = self.committed.next()?;
        self.attempt =
            Some(IceRestartAttempt { generation, deadline, engaged: false, authorized: false });
        Ok(generation)
    }

    pub(crate) fn begin_remote(
        &mut self,
        generation: TransportGeneration,
        deadline: Instant,
    ) -> Result<(), PeerSessionError> {
        if self.attempt.is_some() {
            return Err(PeerSessionError::Protocol("ICE restart is already in progress".into()));
        }
        if generation != self.committed.next()? {
            return Err(PeerSessionError::Protocol(
                "restart did not use the next transport generation".into(),
            ));
        }
        self.attempt =
            Some(IceRestartAttempt { generation, deadline, engaged: false, authorized: false });
        Ok(())
    }

    pub(crate) fn require_active(
        &self,
        generation: TransportGeneration,
    ) -> Result<IceRestartAttempt, PeerSessionError> {
        match self.attempt {
            Some(attempt) if attempt.generation == generation => Ok(attempt),
            Some(_) => Err(PeerSessionError::Protocol(
                "signaling used the wrong transport generation".into(),
            )),
            None => Err(PeerSessionError::Protocol(
                "restart signaling arrived without an active restart".into(),
            )),
        }
    }

    pub(crate) fn engage(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), PeerSessionError> {
        self.require_active(generation)?;
        let attempt = self.attempt.as_mut().expect("active attempt checked above");
        if attempt.engaged {
            return Err(PeerSessionError::Protocol("ICE restart is already engaged".into()));
        }
        attempt.engaged = true;
        Ok(())
    }

    pub(crate) fn authorize(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), PeerSessionError> {
        self.require_active(generation)?;
        let attempt = self.attempt.as_mut().expect("active attempt checked above");
        if !attempt.engaged {
            return Err(PeerSessionError::Protocol(
                "ICE restart was acknowledged before engagement".into(),
            ));
        }
        attempt.authorized = true;
        Ok(())
    }

    pub(crate) fn can_cancel(&self) -> bool {
        self.attempt.is_some_and(|attempt| !attempt.engaged)
    }

    pub(crate) fn cancel(&mut self) -> Result<(), PeerSessionError> {
        if !self.can_cancel() {
            return Err(PeerSessionError::Protocol(
                "authorized ICE restart cannot be cancelled".into(),
            ));
        }
        self.attempt = None;
        Ok(())
    }

    pub(crate) fn commit(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), PeerSessionError> {
        let attempt = self.require_active(generation)?;
        if !attempt.authorized {
            return Err(PeerSessionError::Protocol(
                "cannot commit an unauthorized ICE restart".into(),
            ));
        }
        self.committed = generation;
        self.attempt = None;
        Ok(())
    }
}

impl IceRestartAttempt {
    pub(crate) fn generation(self) -> TransportGeneration {
        self.generation
    }

    pub(crate) fn deadline(self) -> Instant {
        self.deadline
    }

    pub(crate) fn authorized(self) -> bool {
        self.authorized
    }

    pub(crate) fn engaged(self) -> bool {
        self.engaged
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn restart_generation_is_exactly_next_and_commits_once() {
        let now = Instant::now();
        let mut coordinator = IceRestartCoordinator::default();
        let generation = coordinator.begin_local(now + Duration::from_secs(1)).unwrap();
        assert_eq!(generation, TransportGeneration::from_value(1));
        assert!(coordinator.begin_local(now).is_err());
        assert!(coordinator.commit(generation).is_err());
        coordinator.engage(generation).unwrap();
        coordinator.authorize(generation).unwrap();
        coordinator.commit(generation).unwrap();
        assert_eq!(coordinator.committed(), generation);
    }

    #[test]
    fn stale_future_and_duplicate_remote_attempts_are_rejected() {
        let now = Instant::now();
        let mut coordinator = IceRestartCoordinator::default();
        assert!(coordinator.begin_remote(TransportGeneration::from_value(2), now).is_err());
        coordinator.begin_remote(TransportGeneration::from_value(1), now).unwrap();
        assert!(coordinator.begin_remote(TransportGeneration::from_value(1), now).is_err());
        assert!(coordinator.require_active(TransportGeneration::INITIAL).is_err());
    }

    #[test]
    fn only_an_unengaged_attempt_can_be_cancelled() {
        let now = Instant::now();
        let mut coordinator = IceRestartCoordinator::default();
        let generation = coordinator.begin_local(now).unwrap();
        coordinator.cancel().unwrap();
        coordinator
            .begin_remote(generation, now)
            .unwrap_or_else(|error| panic!("cancelled generation remains reusable: {error}"));
        coordinator.engage(TransportGeneration::from_value(1)).unwrap();
        coordinator.authorize(TransportGeneration::from_value(1)).unwrap();
        assert!(coordinator.cancel().is_err());
    }
}

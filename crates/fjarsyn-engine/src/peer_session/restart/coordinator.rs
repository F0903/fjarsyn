use tokio::time::Instant;

use super::Attempt;
use crate::peer_session::{Error, TransportGeneration};

/// Pure restart-attempt ownership and generation fencing. Network and WebRTC
/// side effects remain actor-owned; this value only admits valid transitions.
#[derive(Debug)]
pub(in crate::peer_session) struct Coordinator {
    committed: TransportGeneration,
    attempt: Option<Attempt>,
}

impl Default for Coordinator {
    fn default() -> Self {
        Self { committed: TransportGeneration::INITIAL, attempt: None }
    }
}

impl Coordinator {
    pub(in crate::peer_session) fn committed(&self) -> TransportGeneration {
        self.committed
    }

    pub(in crate::peer_session) fn active(&self) -> Option<Attempt> {
        self.attempt
    }

    pub(in crate::peer_session) fn begin_local(
        &mut self,
        deadline: Instant,
    ) -> Result<TransportGeneration, Error> {
        if self.attempt.is_some() {
            return Err(Error::Protocol("ICE restart is already in progress".into()));
        }
        let generation = self.committed.next()?;
        self.attempt = Some(Attempt::new(generation, deadline));
        Ok(generation)
    }

    pub(in crate::peer_session) fn begin_remote(
        &mut self,
        generation: TransportGeneration,
        deadline: Instant,
    ) -> Result<(), Error> {
        if self.attempt.is_some() {
            return Err(Error::Protocol("ICE restart is already in progress".into()));
        }
        if generation != self.committed.next()? {
            return Err(Error::Protocol(
                "restart did not use the next transport generation".into(),
            ));
        }
        self.attempt = Some(Attempt::new(generation, deadline));
        Ok(())
    }

    pub(in crate::peer_session) fn require_active(
        &self,
        generation: TransportGeneration,
    ) -> Result<Attempt, Error> {
        match self.attempt {
            Some(attempt) if attempt.generation() == generation => Ok(attempt),
            Some(_) => Err(Error::Protocol("signaling used the wrong transport generation".into())),
            None => {
                Err(Error::Protocol("restart signaling arrived without an active restart".into()))
            }
        }
    }

    pub(in crate::peer_session) fn engage(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        self.require_active(generation)?;
        let attempt = self.attempt.as_mut().expect("active attempt checked above");
        if attempt.engaged() {
            return Err(Error::Protocol("ICE restart is already engaged".into()));
        }
        attempt.mark_engaged();
        Ok(())
    }

    pub(in crate::peer_session) fn authorize(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        self.require_active(generation)?;
        let attempt = self.attempt.as_mut().expect("active attempt checked above");
        if !attempt.engaged() {
            return Err(Error::Protocol("ICE restart was acknowledged before engagement".into()));
        }
        attempt.mark_authorized();
        Ok(())
    }

    pub(in crate::peer_session) fn can_cancel(&self) -> bool {
        self.attempt.is_some_and(|attempt| !attempt.engaged())
    }

    pub(in crate::peer_session) fn cancel(&mut self) -> Result<(), Error> {
        if !self.can_cancel() {
            return Err(Error::Protocol("authorized ICE restart cannot be cancelled".into()));
        }
        self.attempt = None;
        Ok(())
    }

    pub(in crate::peer_session) fn commit(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        let attempt = self.require_active(generation)?;
        if !attempt.authorized() {
            return Err(Error::Protocol("cannot commit an unauthorized ICE restart".into()));
        }
        self.committed = generation;
        self.attempt = None;
        Ok(())
    }
}

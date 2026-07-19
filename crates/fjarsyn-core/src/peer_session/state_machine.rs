use super::{PeerSessionPhase, SessionCloseReason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionInput {
    AcceptLocal,
    AcceptRemote,
    TransportReady,
    TransportLost,
    TransportRecovered,
    DisconnectLocal,
    DisconnectRemote,
    RejectLocal(String),
    RejectRemote(String),
    Cancel,
    Fail(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionTransition {
    Phase(PeerSessionPhase),
    Close(SessionCloseReason),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("input {input:?} is invalid while session is {phase:?}")]
pub(crate) struct InvalidTransition {
    pub phase: PeerSessionPhase,
    pub input: SessionInput,
}

#[derive(Debug)]
pub(crate) struct SessionStateMachine {
    phase: PeerSessionPhase,
}

impl SessionStateMachine {
    pub(crate) fn outgoing() -> Self {
        Self { phase: PeerSessionPhase::Requesting }
    }

    pub(crate) fn incoming() -> Self {
        Self { phase: PeerSessionPhase::Incoming }
    }

    pub(crate) fn phase(&self) -> PeerSessionPhase {
        self.phase
    }

    pub(crate) fn apply(
        &mut self,
        input: SessionInput,
    ) -> Result<SessionTransition, InvalidTransition> {
        let transition = match (&self.phase, &input) {
            (PeerSessionPhase::Incoming, SessionInput::AcceptLocal)
            | (PeerSessionPhase::Requesting, SessionInput::AcceptRemote) => {
                SessionTransition::Phase(PeerSessionPhase::Negotiating)
            }
            (PeerSessionPhase::Negotiating, SessionInput::TransportReady) => {
                SessionTransition::Phase(PeerSessionPhase::Connected)
            }
            (PeerSessionPhase::Connected, SessionInput::TransportLost) => {
                SessionTransition::Phase(PeerSessionPhase::Reconnecting)
            }
            (PeerSessionPhase::Reconnecting, SessionInput::TransportRecovered) => {
                SessionTransition::Phase(PeerSessionPhase::Connected)
            }
            (PeerSessionPhase::Incoming, SessionInput::RejectLocal(reason))
            | (PeerSessionPhase::Requesting, SessionInput::RejectRemote(reason)) => {
                SessionTransition::Close(SessionCloseReason::Rejected { reason: reason.clone() })
            }
            (PeerSessionPhase::Requesting, SessionInput::Cancel)
            | (PeerSessionPhase::Incoming, SessionInput::Cancel)
            | (PeerSessionPhase::Negotiating, SessionInput::Cancel) => {
                SessionTransition::Close(SessionCloseReason::Cancelled)
            }
            (_, SessionInput::DisconnectLocal) if self.phase != PeerSessionPhase::Disconnecting => {
                SessionTransition::Phase(PeerSessionPhase::Disconnecting)
            }
            (_, SessionInput::DisconnectRemote)
                if self.phase != PeerSessionPhase::Disconnecting =>
            {
                SessionTransition::Close(SessionCloseReason::RemoteDisconnect)
            }
            (_, SessionInput::Fail(reason)) if self.phase != PeerSessionPhase::Disconnecting => {
                SessionTransition::Close(SessionCloseReason::ConnectionFailed {
                    reason: reason.clone(),
                })
            }
            _ => {
                return Err(InvalidTransition { phase: self.phase, input });
            }
        };

        if let SessionTransition::Phase(next) = transition {
            self.phase = next;
        }
        Ok(transition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_happy_path_is_explicit() {
        let mut machine = SessionStateMachine::outgoing();
        assert_eq!(
            machine.apply(SessionInput::AcceptRemote).unwrap(),
            SessionTransition::Phase(PeerSessionPhase::Negotiating)
        );
        assert_eq!(
            machine.apply(SessionInput::TransportReady).unwrap(),
            SessionTransition::Phase(PeerSessionPhase::Connected)
        );
        assert_eq!(
            machine.apply(SessionInput::DisconnectLocal).unwrap(),
            SessionTransition::Phase(PeerSessionPhase::Disconnecting)
        );
    }

    #[test]
    fn incoming_requires_local_acceptance() {
        let mut machine = SessionStateMachine::incoming();
        assert!(machine.apply(SessionInput::TransportReady).is_err());
        assert_eq!(
            machine.apply(SessionInput::AcceptLocal).unwrap(),
            SessionTransition::Phase(PeerSessionPhase::Negotiating)
        );
    }

    #[test]
    fn connected_session_can_recover_without_becoming_a_new_session() {
        let mut machine = SessionStateMachine { phase: PeerSessionPhase::Connected };
        assert_eq!(
            machine.apply(SessionInput::TransportLost).unwrap(),
            SessionTransition::Phase(PeerSessionPhase::Reconnecting)
        );
        assert_eq!(
            machine.apply(SessionInput::TransportRecovered).unwrap(),
            SessionTransition::Phase(PeerSessionPhase::Connected)
        );
    }

    #[test]
    fn invalid_transition_table_rejects_crossed_roles_and_reuse() {
        let cases = [
            (PeerSessionPhase::Requesting, SessionInput::AcceptLocal),
            (PeerSessionPhase::Incoming, SessionInput::AcceptRemote),
            (PeerSessionPhase::Connected, SessionInput::TransportReady),
            (PeerSessionPhase::Connected, SessionInput::TransportRecovered),
            (PeerSessionPhase::Reconnecting, SessionInput::TransportLost),
            (PeerSessionPhase::Disconnecting, SessionInput::DisconnectLocal),
            (PeerSessionPhase::Disconnecting, SessionInput::Fail("late".into())),
        ];

        for (phase, input) in cases {
            let mut machine = SessionStateMachine { phase };
            assert!(machine.apply(input).is_err(), "phase {phase:?}");
        }
    }

    #[test]
    fn rejection_and_failure_are_terminal_removal_effects() {
        let mut outgoing = SessionStateMachine::outgoing();
        assert!(matches!(
            outgoing.apply(SessionInput::RejectRemote("no".into())).unwrap(),
            SessionTransition::Close(SessionCloseReason::Rejected { .. })
        ));

        let mut incoming = SessionStateMachine::incoming();
        assert!(matches!(
            incoming.apply(SessionInput::RejectLocal("busy".into())).unwrap(),
            SessionTransition::Close(SessionCloseReason::Rejected { .. })
        ));

        let mut incoming = SessionStateMachine::incoming();
        assert!(matches!(
            incoming.apply(SessionInput::Fail("transport".into())).unwrap(),
            SessionTransition::Close(SessionCloseReason::ConnectionFailed { .. })
        ));
    }

    #[test]
    fn every_phase_and_input_category_has_an_explicit_transition_decision() {
        let phases = [
            PeerSessionPhase::Requesting,
            PeerSessionPhase::Incoming,
            PeerSessionPhase::Negotiating,
            PeerSessionPhase::Connected,
            PeerSessionPhase::Reconnecting,
            PeerSessionPhase::Disconnecting,
        ];
        let inputs = || {
            [
                SessionInput::AcceptLocal,
                SessionInput::AcceptRemote,
                SessionInput::TransportReady,
                SessionInput::TransportLost,
                SessionInput::TransportRecovered,
                SessionInput::DisconnectLocal,
                SessionInput::DisconnectRemote,
                SessionInput::RejectLocal("local".into()),
                SessionInput::RejectRemote("remote".into()),
                SessionInput::Cancel,
                SessionInput::Fail("failure".into()),
            ]
        };

        for phase in phases {
            for input in inputs() {
                let expected_valid = matches!(
                    (&phase, &input),
                    (PeerSessionPhase::Requesting, SessionInput::AcceptRemote)
                        | (PeerSessionPhase::Requesting, SessionInput::DisconnectLocal)
                        | (PeerSessionPhase::Requesting, SessionInput::DisconnectRemote)
                        | (PeerSessionPhase::Requesting, SessionInput::RejectRemote(_))
                        | (PeerSessionPhase::Requesting, SessionInput::Cancel)
                        | (PeerSessionPhase::Requesting, SessionInput::Fail(_))
                        | (PeerSessionPhase::Incoming, SessionInput::AcceptLocal)
                        | (PeerSessionPhase::Incoming, SessionInput::DisconnectLocal)
                        | (PeerSessionPhase::Incoming, SessionInput::DisconnectRemote)
                        | (PeerSessionPhase::Incoming, SessionInput::RejectLocal(_))
                        | (PeerSessionPhase::Incoming, SessionInput::Cancel)
                        | (PeerSessionPhase::Incoming, SessionInput::Fail(_))
                        | (PeerSessionPhase::Negotiating, SessionInput::TransportReady)
                        | (PeerSessionPhase::Negotiating, SessionInput::DisconnectLocal)
                        | (PeerSessionPhase::Negotiating, SessionInput::DisconnectRemote)
                        | (PeerSessionPhase::Negotiating, SessionInput::Cancel)
                        | (PeerSessionPhase::Negotiating, SessionInput::Fail(_))
                        | (PeerSessionPhase::Connected, SessionInput::DisconnectLocal)
                        | (PeerSessionPhase::Connected, SessionInput::DisconnectRemote)
                        | (PeerSessionPhase::Connected, SessionInput::Fail(_))
                        | (PeerSessionPhase::Connected, SessionInput::TransportLost)
                        | (PeerSessionPhase::Reconnecting, SessionInput::TransportRecovered)
                        | (PeerSessionPhase::Reconnecting, SessionInput::DisconnectLocal)
                        | (PeerSessionPhase::Reconnecting, SessionInput::DisconnectRemote)
                        | (PeerSessionPhase::Reconnecting, SessionInput::Fail(_))
                );
                let actual = SessionStateMachine { phase }.apply(input.clone()).is_ok();
                assert_eq!(actual, expected_valid, "phase={phase:?}, input={input:?}");
            }
        }
    }
}

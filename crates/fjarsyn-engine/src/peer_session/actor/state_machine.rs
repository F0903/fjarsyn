//! Session lifecycle transitions owned by the session actor.

use crate::peer_session::{CloseReason, Phase};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::peer_session::actor) enum Input {
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
pub(in crate::peer_session::actor) enum Transition {
    Phase(Phase),
    Close(CloseReason),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("input {input:?} is invalid while session is {phase:?}")]
pub(in crate::peer_session::actor) struct InvalidTransition {
    pub phase: Phase,
    pub input: Input,
}

#[derive(Debug)]
pub(in crate::peer_session::actor) struct StateMachine {
    phase: Phase,
}

impl StateMachine {
    pub(in crate::peer_session::actor) fn outgoing() -> Self {
        Self { phase: Phase::Requesting }
    }

    pub(in crate::peer_session::actor) fn incoming() -> Self {
        Self { phase: Phase::Incoming }
    }

    #[cfg(test)]
    fn with_phase(phase: Phase) -> Self {
        Self { phase }
    }

    pub(in crate::peer_session::actor) fn phase(&self) -> Phase {
        self.phase
    }

    pub(in crate::peer_session::actor) fn apply(
        &mut self,
        input: Input,
    ) -> Result<Transition, InvalidTransition> {
        let transition = match (&self.phase, &input) {
            (Phase::Incoming, Input::AcceptLocal) | (Phase::Requesting, Input::AcceptRemote) => {
                Transition::Phase(Phase::Negotiating)
            }
            (Phase::Negotiating, Input::TransportReady) => Transition::Phase(Phase::Connected),
            (Phase::Connected, Input::TransportLost) => Transition::Phase(Phase::Reconnecting),
            (Phase::Reconnecting, Input::TransportRecovered) => Transition::Phase(Phase::Connected),
            (Phase::Incoming, Input::RejectLocal(reason))
            | (Phase::Requesting, Input::RejectRemote(reason)) => {
                Transition::Close(CloseReason::Rejected { reason: reason.clone() })
            }
            (Phase::Requesting, Input::Cancel)
            | (Phase::Incoming, Input::Cancel)
            | (Phase::Negotiating, Input::Cancel) => Transition::Close(CloseReason::Cancelled),
            (_, Input::DisconnectLocal) if self.phase != Phase::Disconnecting => {
                Transition::Phase(Phase::Disconnecting)
            }
            (_, Input::DisconnectRemote) if self.phase != Phase::Disconnecting => {
                Transition::Close(CloseReason::RemoteDisconnect)
            }
            (_, Input::Fail(reason)) if self.phase != Phase::Disconnecting => {
                Transition::Close(CloseReason::ConnectionFailed { reason: reason.clone() })
            }
            _ => return Err(InvalidTransition { phase: self.phase, input }),
        };

        if let Transition::Phase(next) = transition {
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
        let mut machine = StateMachine::outgoing();
        assert_eq!(
            machine.apply(Input::AcceptRemote).unwrap(),
            Transition::Phase(Phase::Negotiating)
        );
        assert_eq!(
            machine.apply(Input::TransportReady).unwrap(),
            Transition::Phase(Phase::Connected)
        );
        assert_eq!(
            machine.apply(Input::DisconnectLocal).unwrap(),
            Transition::Phase(Phase::Disconnecting)
        );
    }

    #[test]
    fn incoming_requires_local_acceptance() {
        let mut machine = StateMachine::incoming();
        assert!(machine.apply(Input::TransportReady).is_err());
        assert_eq!(
            machine.apply(Input::AcceptLocal).unwrap(),
            Transition::Phase(Phase::Negotiating)
        );
    }

    #[test]
    fn connected_session_can_recover_without_becoming_a_new_session() {
        let mut machine = StateMachine::with_phase(Phase::Connected);
        assert_eq!(
            machine.apply(Input::TransportLost).unwrap(),
            Transition::Phase(Phase::Reconnecting)
        );
        assert_eq!(
            machine.apply(Input::TransportRecovered).unwrap(),
            Transition::Phase(Phase::Connected)
        );
    }

    #[test]
    fn invalid_transition_table_rejects_crossed_roles_and_reuse() {
        let cases = [
            (Phase::Requesting, Input::AcceptLocal),
            (Phase::Incoming, Input::AcceptRemote),
            (Phase::Connected, Input::TransportReady),
            (Phase::Connected, Input::TransportRecovered),
            (Phase::Reconnecting, Input::TransportLost),
            (Phase::Disconnecting, Input::DisconnectLocal),
            (Phase::Disconnecting, Input::Fail("late".into())),
        ];

        for (phase, input) in cases {
            let mut machine = StateMachine::with_phase(phase);
            assert!(machine.apply(input).is_err(), "phase {phase:?}");
        }
    }

    #[test]
    fn rejection_and_failure_are_terminal_removal_effects() {
        let mut outgoing = StateMachine::outgoing();
        assert!(matches!(
            outgoing.apply(Input::RejectRemote("no".into())).unwrap(),
            Transition::Close(CloseReason::Rejected { .. })
        ));

        let mut incoming = StateMachine::incoming();
        assert!(matches!(
            incoming.apply(Input::RejectLocal("busy".into())).unwrap(),
            Transition::Close(CloseReason::Rejected { .. })
        ));

        let mut incoming = StateMachine::incoming();
        assert!(matches!(
            incoming.apply(Input::Fail("transport".into())).unwrap(),
            Transition::Close(CloseReason::ConnectionFailed { .. })
        ));
    }

    #[test]
    fn every_phase_and_input_category_has_an_explicit_transition_decision() {
        let phases = [
            Phase::Requesting,
            Phase::Incoming,
            Phase::Negotiating,
            Phase::Connected,
            Phase::Reconnecting,
            Phase::Disconnecting,
        ];
        let inputs = || {
            [
                Input::AcceptLocal,
                Input::AcceptRemote,
                Input::TransportReady,
                Input::TransportLost,
                Input::TransportRecovered,
                Input::DisconnectLocal,
                Input::DisconnectRemote,
                Input::RejectLocal("local".into()),
                Input::RejectRemote("remote".into()),
                Input::Cancel,
                Input::Fail("failure".into()),
            ]
        };

        for phase in phases {
            for input in inputs() {
                let expected_valid = matches!(
                    (&phase, &input),
                    (Phase::Requesting, Input::AcceptRemote)
                        | (Phase::Requesting, Input::DisconnectLocal)
                        | (Phase::Requesting, Input::DisconnectRemote)
                        | (Phase::Requesting, Input::RejectRemote(_))
                        | (Phase::Requesting, Input::Cancel)
                        | (Phase::Requesting, Input::Fail(_))
                        | (Phase::Incoming, Input::AcceptLocal)
                        | (Phase::Incoming, Input::DisconnectLocal)
                        | (Phase::Incoming, Input::DisconnectRemote)
                        | (Phase::Incoming, Input::RejectLocal(_))
                        | (Phase::Incoming, Input::Cancel)
                        | (Phase::Incoming, Input::Fail(_))
                        | (Phase::Negotiating, Input::TransportReady)
                        | (Phase::Negotiating, Input::DisconnectLocal)
                        | (Phase::Negotiating, Input::DisconnectRemote)
                        | (Phase::Negotiating, Input::Cancel)
                        | (Phase::Negotiating, Input::Fail(_))
                        | (Phase::Connected, Input::DisconnectLocal)
                        | (Phase::Connected, Input::DisconnectRemote)
                        | (Phase::Connected, Input::Fail(_))
                        | (Phase::Connected, Input::TransportLost)
                        | (Phase::Reconnecting, Input::TransportRecovered)
                        | (Phase::Reconnecting, Input::DisconnectLocal)
                        | (Phase::Reconnecting, Input::DisconnectRemote)
                        | (Phase::Reconnecting, Input::Fail(_))
                );
                let actual = StateMachine::with_phase(phase).apply(input.clone()).is_ok();
                assert_eq!(actual, expected_valid, "phase={phase:?}, input={input:?}");
            }
        }
    }
}

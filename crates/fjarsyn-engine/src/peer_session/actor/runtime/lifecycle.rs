use tokio::time::Instant;

use super::{
    super::{Update, state_machine},
    Runtime,
};
use crate::peer_session::{
    CloseReason, Error, Event, Phase, SessionState, protocol::NegotiationSignal, rtc::Peer,
};

impl Runtime {
    pub(super) async fn ensure_rtc(&mut self) -> Result<(), Error> {
        if self.rtc.is_none() {
            self.rtc = Some(
                Peer::new(
                    self.config.rtc.clone(),
                    self.rtc_event_tx.clone(),
                    self.rtc_fatal_tx.clone(),
                    self.remote_video_tx.clone(),
                )
                .await?,
            );
        }
        Ok(())
    }

    pub(super) async fn send_signal(&self, signal: NegotiationSignal) -> Result<(), Error> {
        let sending = self
            .connection
            .as_ref()
            .ok_or_else(|| Error::Signaling("signaling connection is closed".into()))?
            .send(signal);
        if let Some(attempt) = self.restart.active()
            && self.state.phase() == Phase::Reconnecting
        {
            tokio::time::timeout_at(attempt.deadline(), sending)
                .await
                .map_err(|_| Error::OperationTimeout)?
        } else {
            sending.await
        }
    }

    pub(super) async fn apply(&mut self, input: state_machine::Input) -> Result<(), Error> {
        let transition = self.state.apply(input).map_err(|error| Error::InvalidState {
            session_id: self.config.session_id,
            phase: error.phase.name(),
            operation: "apply session transition",
        })?;
        match transition {
            state_machine::Transition::Phase(_) => {
                self.phase_started = Instant::now();
                self.publish_snapshot().await;
            }
            state_machine::Transition::Close(reason) => self.terminal_reason = Some(reason),
        }
        Ok(())
    }

    pub(super) fn require_connected(&self, operation: &'static str) -> Result<(), Error> {
        if self.state.phase() == Phase::Connected {
            Ok(())
        } else {
            Err(Error::InvalidState {
                session_id: self.config.session_id,
                phase: self.state.phase().name(),
                operation,
            })
        }
    }

    pub(super) fn invalid_state(&self, operation: &'static str) -> Error {
        Error::InvalidState {
            session_id: self.config.session_id,
            phase: self.state.phase().name(),
            operation,
        }
    }

    pub(super) fn fail(&mut self, reason: String) {
        let _ = self.state.apply(state_machine::Input::Fail(reason.clone()));
        self.terminal_reason = Some(CloseReason::ConnectionFailed { reason });
    }

    pub(super) fn fail_error(&mut self, error: Error) {
        let reason = error.to_string();
        let _ = self.state.apply(state_machine::Input::Fail(reason.clone()));
        self.terminal_reason = Some(match error {
            Error::Protocol(_) | Error::ShareMismatch(_) => {
                CloseReason::ProtocolViolation { reason }
            }
            _ => CloseReason::ConnectionFailed { reason },
        });
    }

    pub(super) fn fail_on_terminal_command_error<T>(&mut self, result: &Result<T, Error>) {
        if let Err(error) = result
            && command_error_is_terminal(error, true)
        {
            self.fail_error(error.clone());
        }
    }

    pub(super) fn fail_on_message_command_error<T>(&mut self, result: &Result<T, Error>) {
        if let Err(error) = result
            && command_error_is_terminal(error, false)
        {
            self.fail_error(error.clone());
        }
    }

    pub(super) async fn publish_snapshot(&self) {
        let snapshot = SessionState {
            session_id: self.config.session_id,
            peer_id: self.config.remote_peer_id.clone(),
            phase: self.state.phase(),
            local_share: self.local_share,
            remote_share: self.remote_share,
        };
        self.snapshot_tx.send_replace(snapshot);
        // The actor handle's watch channel is the coalescing source of truth.
        // The service periodically projects all actor watches into its snapshot.
    }

    pub(super) async fn emit(&mut self, event: Event) {
        let update = Update { instance_id: self.instance_id, event };
        match tokio::time::timeout(self.config.event_delivery_timeout, self.update_tx.send(update))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(_)) => self.fail("peer-session event receiver closed".into()),
            Err(_) => self.fail("peer-session event delivery timed out".into()),
        }
    }
}

fn command_error_is_terminal(error: &Error, outcome_unknown_is_terminal: bool) -> bool {
    !matches!(
        error,
        Error::InvalidState { .. }
            | Error::EmptyMessage
            | Error::MessageTooLarge { .. }
            | Error::ShareMismatch(_)
    ) && (outcome_unknown_is_terminal || !matches!(error, Error::OutcomeUnknown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_message_send_keeps_session_open_for_receipt_reconciliation() {
        assert!(!command_error_is_terminal(&Error::OutcomeUnknown, false));
        assert!(command_error_is_terminal(&Error::OutcomeUnknown, true));
        assert!(command_error_is_terminal(&Error::WebRtc("transport failed".into()), false));
    }
}

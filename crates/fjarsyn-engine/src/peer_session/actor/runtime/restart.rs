use tokio::time::Instant;

use super::{
    super::{Role, restart::DialResult, state_machine},
    Runtime,
};
use crate::peer_session::{
    Error, Phase, TransportGeneration, negotiation, protocol::NegotiationSignal,
};

impl Runtime {
    pub(super) async fn begin_ice_restart(&mut self) -> Result<(), Error> {
        self.begin_ice_restart_with_old_transport_recovery(true).await
    }

    pub(super) async fn begin_ice_restart_with_old_transport_recovery(
        &mut self,
        allowed: bool,
    ) -> Result<(), Error> {
        if self.state.phase() == Phase::Reconnecting {
            return Ok(());
        }
        if self.state.phase() != Phase::Connected {
            return Err(self.invalid_state("restart ICE"));
        }

        let deadline = Instant::now() + self.config.ice_restart_timeout;
        let generation = self.restart.begin_local(deadline, allowed)?;
        self.keyframe_requests.request();
        self.apply(state_machine::Input::TransportLost).await?;
        self.application_data.reset_readiness();
        self.disconnected_since = None;

        self.restart.spawn_dial(
            self.config.negotiation.clone(),
            self.config.session_id,
            self.config.remote_peer_id.clone(),
            generation,
            deadline,
        );
        Ok(())
    }

    pub(super) async fn handle_restart_dial_result(&mut self, dial: DialResult) {
        let active_generation = self.restart.active().map(|attempt| attempt.generation());
        if self.state.phase() != Phase::Reconnecting || active_generation != Some(dial.generation) {
            if let Ok(connection) = dial.result {
                connection.shutdown_until(Instant::now() + self.config.cleanup_timeout).await;
            }
            return;
        }
        let deadline =
            self.restart.active().expect("active restart generation checked above").deadline();

        match dial.result {
            Ok(connection) => {
                if connection.authenticated_remote_public_key() != self.config.remote_public_key {
                    connection.shutdown_until(deadline).await;
                    self.fail("peer identity changed during ICE restart".into());
                    return;
                }
                let sent = tokio::time::timeout_at(
                    deadline,
                    connection.send(NegotiationSignal::Restart { generation: dial.generation }),
                )
                .await
                .map_err(|_| Error::OperationTimeout)
                .and_then(|result| result);
                if let Err(error) = sent {
                    connection.shutdown_until(deadline).await;
                    self.fail_error(error);
                    return;
                }
                if let Err(error) = self.restart.engage(dial.generation) {
                    connection.shutdown_until(deadline).await;
                    self.fail_error(error);
                    return;
                }
                self.connection = Some(connection);
                self.restart.mark_connection_dialed();
            }
            Err(error) => self.fail_error(error),
        }
    }

    pub(super) async fn attach_restart(
        &mut self,
        generation: TransportGeneration,
        connection: negotiation::Connection,
    ) {
        if connection.authenticated_remote_public_key() != self.config.remote_public_key {
            self.reject_restart_connection(connection, "restart peer identity changed");
            return;
        }
        if !matches!(self.state.phase(), Phase::Connected | Phase::Reconnecting) {
            self.reject_restart_connection(connection, "restart signaling is not expected");
            return;
        }
        if self.restart.conflicts_with_canonical_connection(self.config.role) {
            self.reject_restart_connection(connection, "another restart signaling path won");
            return;
        }

        let newly_started = self.state.phase() == Phase::Connected;
        let admission = if newly_started {
            let deadline = Instant::now() + self.config.ice_restart_timeout;
            match self.restart.begin_remote(generation, deadline) {
                Ok(()) => {
                    self.keyframe_requests.request();
                    self.apply(state_machine::Input::TransportLost).await
                }
                Err(error) => Err(error),
            }
        } else {
            self.restart.require_active(generation).map(|_| ())
        };
        if let Err(error) = admission {
            self.reject_restart_connection(connection, error.to_string());
            return;
        }

        let attempt = match self.restart.require_active(generation) {
            Ok(attempt) => attempt,
            Err(error) => {
                self.reject_restart_connection(connection, error.to_string());
                return;
            }
        };
        let deadline = attempt.deadline();
        if self.config.role == Role::Incoming {
            self.restart.abort_dial(deadline).await;
            if self.restart.connection_was_dialed() {
                if let Some(previous) = self.connection.take() {
                    previous.shutdown_until(deadline).await;
                }
                self.restart.clear_connection_origin();
            }
        }

        self.application_data.reset_readiness();
        self.disconnected_since = None;
        if !attempt.engaged()
            && let Err(error) = self.restart.engage(generation)
        {
            connection.shutdown_until(deadline).await;
            self.fail_error(error);
            return;
        }
        let acknowledged = tokio::time::timeout_at(
            deadline,
            connection.send(NegotiationSignal::RestartAck { generation }),
        )
        .await
        .map_err(|_| Error::OperationTimeout)
        .and_then(|result| result);
        if let Err(error) = acknowledged {
            connection.shutdown_until(deadline).await;
            self.fail_error(error);
            return;
        }
        if let Err(error) = self.restart.authorize(generation) {
            connection.shutdown_until(deadline).await;
            self.fail_error(error);
            return;
        }
        self.connection = Some(connection);
        self.restart.mark_connection_attached();
        if self.config.role == Role::Outgoing
            && let Err(error) = self.create_and_send_restart_offer(generation).await
        {
            self.fail_error(error);
        }
    }

    fn reject_restart_connection(
        &mut self,
        connection: negotiation::Connection,
        reason: impl Into<String>,
    ) {
        self.restart.reject_connection(connection, reason, self.config.cleanup_timeout);
    }

    pub(super) async fn handle_restart_ack(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        if self.state.phase() != Phase::Reconnecting {
            return Err(self.invalid_state("acknowledge an ICE restart"));
        }
        let attempt = self.restart.require_active(generation)?;
        if attempt.authorized() {
            return Err(Error::Protocol("duplicate ICE restart acknowledgement".into()));
        }
        self.restart.authorize(generation)?;
        if self.config.role == Role::Outgoing {
            self.create_and_send_restart_offer(generation).await
        } else {
            Ok(())
        }
    }

    async fn create_and_send_restart_offer(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        let deadline = self.restart.require_active(generation)?.deadline();
        let creating_offer = self
            .rtc
            .as_mut()
            .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?
            .create_restart_offer(generation);
        let sdp = tokio::time::timeout_at(deadline, creating_offer)
            .await
            .map_err(|_| Error::OperationTimeout)??;
        self.send_signal(NegotiationSignal::Offer { generation, sdp }).await
    }

    pub(super) fn require_transport_generation(
        &self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        self.restart.require_transport_generation(self.state.phase(), generation)
    }
}

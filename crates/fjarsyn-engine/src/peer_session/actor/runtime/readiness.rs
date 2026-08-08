use tokio::time::Instant;

use super::{super::state_machine, Runtime};
use crate::peer_session::{Error, Event, Phase, TransportGeneration, protocol::NegotiationSignal};

impl Runtime {
    pub(super) async fn try_announce_ready(&mut self) -> Result<(), Error> {
        if self.application_data.should_announce_ready(self.state.phase()) {
            self.application_data.mark_local_ready();
            let generation = self.active_transport_generation()?;
            self.send_signal(NegotiationSignal::Ready { generation }).await?;
            self.try_finish_ready().await?;
        }
        Ok(())
    }

    pub(super) async fn handle_remote_ready(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        self.require_transport_generation(generation)?;
        self.application_data.accept_remote_ready(self.state.phase())?;
        // `negotiation::Connection::send` resolves only after the frame is written,
        // so closing signaling after this point cannot discard the acknowledgement.
        self.send_signal(NegotiationSignal::ReadyAck { generation }).await?;
        self.try_finish_ready().await
    }

    pub(super) async fn handle_ready_ack(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        self.require_transport_generation(generation)?;
        self.application_data.accept_ready_acknowledgement(self.state.phase())?;
        self.try_finish_ready().await
    }

    async fn try_finish_ready(&mut self) -> Result<(), Error> {
        if self.application_data.handshake_complete(self.state.phase()) {
            let initial = self.state.phase() == Phase::Negotiating;
            let restart_deadline = if initial {
                None
            } else {
                let generation = self.active_transport_generation()?;
                Some(self.restart.require_active(generation)?.deadline())
            };
            if let Some(connection) = self.connection.take() {
                match restart_deadline {
                    Some(deadline) => connection.shutdown_until(deadline).await,
                    None => connection.shutdown().await,
                }
            }
            self.restart.clear_connection_origin();
            if restart_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(Error::OperationTimeout);
            }
            if let Some(deadline) = restart_deadline {
                let generation = self.active_transport_generation()?;
                let operational = tokio::time::timeout_at(
                    deadline,
                    self.rtc
                        .as_ref()
                        .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?
                        .transport_is_operational(generation),
                )
                .await
                .map_err(|_| Error::OperationTimeout)??;
                if !operational {
                    return Err(Error::WebRtc(
                        "transport was lost before ICE restart commit".into(),
                    ));
                }
            }
            if initial {
                self.apply(state_machine::Input::TransportReady).await?;
            } else {
                let generation = self.active_transport_generation()?;
                self.restart.commit(generation)?;
                self.apply(state_machine::Input::TransportRecovered).await?;
            }
            if initial {
                self.emit(Event::Connected {
                    session_id: self.config.session_id,
                    peer_id: self.config.remote_peer_id.clone(),
                })
                .await;
            }
            self.flush_pending_application_data().await?;
        }
        Ok(())
    }

    fn active_transport_generation(&self) -> Result<TransportGeneration, Error> {
        self.restart.active_transport_generation(self.state.phase())
    }
}

use super::{
    super::{Role, state_machine},
    Runtime,
};
use crate::peer_session::{
    CloseReason, Error, Phase, TransportGeneration, protocol::NegotiationSignal,
};

impl Runtime {
    pub(super) async fn handle_signal(&mut self, signal: NegotiationSignal) {
        let result = match signal {
            NegotiationSignal::EndpointHello { .. } | NegotiationSignal::EndpointProof { .. } => {
                Err(Error::Protocol(
                    "endpoint-authentication message received after signaling authentication"
                        .into(),
                ))
            }
            NegotiationSignal::Request {} => {
                Err(Error::Protocol("duplicate connection request".into()))
            }
            NegotiationSignal::Restart { .. } => {
                Err(Error::Protocol("restart intent received after signaling routing".into()))
            }
            NegotiationSignal::RestartAck { generation } => {
                self.handle_restart_ack(generation).await
            }
            NegotiationSignal::Accept {} => self.handle_remote_accept().await,
            NegotiationSignal::Offer { generation, sdp } => {
                self.handle_offer(generation, sdp).await
            }
            NegotiationSignal::Answer { generation, sdp } => {
                self.handle_answer(generation, sdp).await
            }
            NegotiationSignal::IceCandidate { generation, candidate } => {
                match self.require_transport_generation(generation) {
                    Ok(()) => {
                        async {
                            let restart_deadline = if self.state.phase() == Phase::Reconnecting {
                                Some(self.restart.require_active(generation)?.deadline())
                            } else {
                                None
                            };
                            match self.rtc.as_mut() {
                                Some(rtc) => {
                                    let adding = rtc.add_remote_candidate(generation, candidate);
                                    match restart_deadline {
                                        Some(deadline) => tokio::time::timeout_at(deadline, adding)
                                            .await
                                            .map_err(|_| Error::OperationTimeout)?,
                                        None => adding.await,
                                    }
                                }
                                None => Err(Error::Protocol(
                                    "ICE candidate arrived before acceptance".into(),
                                )),
                            }
                        }
                        .await
                    }
                    Err(error) => Err(error),
                }
            }
            NegotiationSignal::Ready { generation } => self.handle_remote_ready(generation).await,
            NegotiationSignal::ReadyAck { generation } => self.handle_ready_ack(generation).await,
            NegotiationSignal::Reject { reason } => {
                if reason.len() > 512 {
                    return self
                        .fail_error(Error::Protocol("rejection reason exceeds limit".into()));
                }
                match self.apply(state_machine::Input::RejectRemote(reason.clone())).await {
                    Ok(()) => self.terminal_reason = Some(CloseReason::Rejected { reason }),
                    Err(error) => return self.fail(error.to_string()),
                }
                Ok(())
            }
            NegotiationSignal::Cancel {} => {
                match self.apply(state_machine::Input::Cancel).await {
                    Ok(()) => self.terminal_reason = Some(CloseReason::Cancelled),
                    Err(error) => return self.fail(error.to_string()),
                }
                Ok(())
            }
        };
        if let Err(error) = result {
            self.fail_error(error);
        }
    }

    async fn handle_remote_accept(&mut self) -> Result<(), Error> {
        self.apply(state_machine::Input::AcceptRemote).await?;
        self.ensure_rtc().await?;
        let rtc = self.rtc.as_mut().expect("RTC initialized above");
        rtc.prepare_offerer_channels().await?;
        let sdp = rtc.create_offer().await?;
        self.send_signal(NegotiationSignal::Offer { generation: TransportGeneration::INITIAL, sdp })
            .await
    }

    async fn handle_offer(
        &mut self,
        generation: TransportGeneration,
        sdp: String,
    ) -> Result<(), Error> {
        self.require_transport_generation(generation)?;
        if self.config.role != Role::Incoming {
            return Err(self.invalid_state("apply a remote offer"));
        }
        let phase = self.state.phase();
        let restart_deadline = if phase == Phase::Reconnecting {
            let attempt = self.restart.require_active(generation)?;
            if !attempt.authorized() {
                return Err(Error::Protocol(
                    "restart offer arrived before restart acknowledgement".into(),
                ));
            }
            Some(attempt.deadline())
        } else {
            None
        };
        let rtc = self
            .rtc
            .as_mut()
            .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?;
        let answer = match phase {
            Phase::Negotiating => rtc.apply_offer_and_create_answer(sdp).await?,
            Phase::Reconnecting => tokio::time::timeout_at(
                restart_deadline.expect("restart deadline checked above"),
                rtc.apply_restart_offer_and_create_answer(generation, sdp),
            )
            .await
            .map_err(|_| Error::OperationTimeout)??,
            _ => return Err(self.invalid_state("apply a remote offer")),
        };
        if phase == Phase::Reconnecting {
            self.restart.mark_sdp_applied();
        }
        self.send_signal(NegotiationSignal::Answer { generation, sdp: answer }).await
    }

    async fn handle_answer(
        &mut self,
        generation: TransportGeneration,
        sdp: String,
    ) -> Result<(), Error> {
        self.require_transport_generation(generation)?;
        if self.config.role != Role::Outgoing {
            return Err(self.invalid_state("apply a remote answer"));
        }
        let phase = self.state.phase();
        let restart_deadline = if phase == Phase::Reconnecting {
            Some(self.restart.require_active(generation)?.deadline())
        } else {
            None
        };
        let rtc = self
            .rtc
            .as_mut()
            .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?;
        let applied = match phase {
            Phase::Negotiating => rtc.apply_answer(sdp).await,
            Phase::Reconnecting => tokio::time::timeout_at(
                restart_deadline.expect("restart deadline checked above"),
                rtc.apply_restart_answer(generation, sdp),
            )
            .await
            .map_err(|_| Error::OperationTimeout)?,
            _ => Err(self.invalid_state("apply a remote answer")),
        };
        applied?;
        if phase == Phase::Reconnecting {
            self.restart.mark_sdp_applied();
        }
        Ok(())
    }
}

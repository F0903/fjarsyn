use tokio::time::Instant;
use webrtc::{
    dtls_transport::dtls_transport_state::RTCDtlsTransportState,
    ice_transport::ice_connection_state::RTCIceConnectionState,
    peer_connection::peer_connection_state::RTCPeerConnectionState,
};

use super::{super::state_machine, Runtime};
use crate::peer_session::{
    Error, Phase, TransportGeneration,
    protocol::NegotiationSignal,
    rtc::{self, ChannelKind},
};

impl Runtime {
    pub(super) async fn handle_rtc_event(&mut self, event: rtc::Event) {
        let result = match event {
            rtc::Event::LocalCandidate { generation, candidate } => {
                if should_forward_local_candidate(
                    self.state.phase(),
                    self.connection.is_some(),
                    self.transport_generation_is_current(generation),
                ) {
                    let prepared = self
                        .rtc
                        .as_ref()
                        .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))
                        .and_then(|rtc| rtc.prepare_local_candidate(generation, candidate));
                    match prepared {
                        Ok(candidate) => {
                            self.send_signal(NegotiationSignal::IceCandidate {
                                generation,
                                candidate,
                            })
                            .await
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    // Trickle ICE callbacks may arrive after the readiness handshake has
                    // deliberately closed signaling. They cannot affect an established session.
                    Ok(())
                }
            }
            rtc::Event::IceState { generation, state } => {
                self.handle_ice_state(generation, state).await
            }
            rtc::Event::DtlsState { generation, state } => {
                if self.transport_event_is_relevant(generation)
                    && self.rtc.as_ref().is_some_and(|rtc| rtc.dtls_state() == state)
                    && matches!(
                        state,
                        RTCDtlsTransportState::Failed | RTCDtlsTransportState::Closed
                    )
                {
                    self.fail(format!("DTLS transport became {state}"));
                }
                Ok(())
            }
            rtc::Event::PeerState { generation, state } => {
                if !self.transport_event_is_relevant(generation)
                    || !self.rtc.as_ref().is_some_and(|rtc| rtc.peer_connection_state() == state)
                {
                    Ok(())
                } else {
                    match state {
                        RTCPeerConnectionState::Connected => {
                            self.handle_transport_connected(generation).await
                        }
                        RTCPeerConnectionState::Disconnected => {
                            self.application_data.mark_peer_disconnected();
                            if self.state.phase() == Phase::Negotiating {
                                self.fail("peer connection disconnected during negotiation".into());
                            } else if self.state.phase() == Phase::Connected {
                                self.disconnected_since.get_or_insert_with(Instant::now);
                            }
                            Ok(())
                        }
                        RTCPeerConnectionState::Failed => {
                            self.handle_current_failed_transport(generation).await
                        }
                        RTCPeerConnectionState::Closed => {
                            self.fail("peer connection became closed".into());
                            Ok(())
                        }
                        _ => Ok(()),
                    }
                }
            }
            rtc::Event::DataChannel(channel) => match self.rtc.as_mut() {
                Some(rtc) => rtc.attach_data_channel(channel),
                None => Err(Error::WebRtc("peer connection is unavailable".into())),
            },
            rtc::Event::ChannelOpen(kind) => {
                self.application_data.set_channel_open(kind, true);
                self.try_announce_ready().await
            }
            rtc::Event::ChannelClosed(kind) => {
                self.application_data.set_channel_open(kind, false);
                if channel_close_is_terminal(self.state.phase()) {
                    self.fail(format!(
                        "{} data channel closed",
                        match kind {
                            ChannelKind::Control => "control",
                            ChannelKind::Messaging => "messaging",
                        }
                    ));
                }
                Ok(())
            }
            rtc::Event::ChannelMessage(kind, data) => self.route_channel_message(kind, data).await,
            rtc::Event::RemoteTrack(track, transceiver) => match self.rtc.as_mut() {
                Some(rtc) => rtc.start_remote_track(track, transceiver),
                None => Err(Error::WebRtc("peer connection is unavailable".into())),
            },
            rtc::Event::Error(reason) => Err(Error::WebRtc(reason)),
            rtc::Event::ProtocolError(reason) => Err(Error::Protocol(reason)),
        };
        if let Err(error) = result {
            self.fail_error(error);
        }
    }

    async fn handle_ice_state(
        &mut self,
        generation: TransportGeneration,
        state: RTCIceConnectionState,
    ) -> Result<(), Error> {
        if !self.transport_event_is_relevant(generation)
            || !self.rtc.as_ref().is_some_and(|rtc| rtc.ice_connection_state() == state)
        {
            return Ok(());
        }
        match state {
            RTCIceConnectionState::Connected | RTCIceConnectionState::Completed => {
                self.handle_transport_connected(generation).await
            }
            RTCIceConnectionState::Disconnected => {
                self.application_data.mark_peer_disconnected();
                if self.state.phase() == Phase::Negotiating {
                    self.fail("ICE disconnected during negotiation".into());
                } else if self.state.phase() == Phase::Connected {
                    self.disconnected_since.get_or_insert_with(Instant::now);
                }
                Ok(())
            }
            RTCIceConnectionState::Failed => {
                self.application_data.mark_peer_disconnected();
                match self.state.phase() {
                    Phase::Connected => self.begin_ice_restart().await,
                    Phase::Negotiating => {
                        self.fail("ICE failed during negotiation".into());
                        Ok(())
                    }
                    Phase::Reconnecting => Ok(()),
                    _ => Ok(()),
                }
            }
            RTCIceConnectionState::Closed => {
                self.fail("ICE transport became closed".into());
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn handle_current_failed_transport(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        let rtc = self
            .rtc
            .as_ref()
            .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?;
        let ice_state = rtc.ice_connection_state();
        let dtls_state = rtc.dtls_state();
        if matches!(dtls_state, RTCDtlsTransportState::Failed | RTCDtlsTransportState::Closed) {
            self.fail(format!("DTLS transport became {dtls_state}"));
            return Ok(());
        }
        if ice_state == RTCIceConnectionState::Failed {
            return self.handle_ice_state(generation, ice_state).await;
        }
        self.fail("peer connection failed without a recoverable ICE failure".into());
        Ok(())
    }

    async fn handle_transport_connected(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        if self.state.phase() == Phase::Reconnecting
            && generation == self.restart.committed()
            && self.restart.can_cancel()
            && self.restart.old_transport_recovery_allowed()
        {
            let deadline = self
                .restart
                .active()
                .expect("cancellable restart has an active attempt")
                .deadline();
            let transport_is_operational = self.application_data.transport_channels_open()
                && tokio::time::timeout_at(
                    deadline,
                    self.rtc
                        .as_ref()
                        .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?
                        .transport_is_operational(generation),
                )
                .await
                .map_err(|_| Error::OperationTimeout)??;
            return if transport_is_operational {
                self.cancel_unengaged_restart().await
            } else {
                Ok(())
            };
        }
        if !self.transport_generation_is_current(generation) {
            return Ok(());
        }
        if self.state.phase() == Phase::Reconnecting {
            if !self.restart.sdp_applied() {
                return Ok(());
            }
            let deadline = self.restart.require_active(generation)?.deadline();
            let ready = tokio::time::timeout_at(
                deadline,
                self.rtc
                    .as_ref()
                    .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?
                    .transport_is_operational(generation),
            )
            .await
            .map_err(|_| Error::OperationTimeout)??;
            if !ready {
                return Ok(());
            }
        }
        self.application_data.mark_peer_connected();
        self.disconnected_since = None;
        self.try_announce_ready().await
    }

    async fn cancel_unengaged_restart(&mut self) -> Result<(), Error> {
        let deadline = self
            .restart
            .active()
            .ok_or_else(|| Error::Protocol("missing ICE restart attempt".into()))?
            .deadline();
        self.restart.abort_dial(deadline).await;
        if let Some(connection) = self.connection.take() {
            connection.shutdown_until(deadline).await;
        }
        self.restart.clear_connection_origin();
        if Instant::now() >= deadline {
            return Err(Error::OperationTimeout);
        }
        let committed = self.restart.committed();
        let operational = tokio::time::timeout_at(
            deadline,
            self.rtc
                .as_ref()
                .ok_or_else(|| Error::WebRtc("peer connection is unavailable".into()))?
                .transport_is_operational(committed),
        )
        .await
        .map_err(|_| Error::OperationTimeout)??;
        if !operational {
            return Err(Error::WebRtc("transport was lost while cancelling ICE recovery".into()));
        }
        self.restart.cancel()?;
        self.application_data.mark_established();
        self.apply(state_machine::Input::TransportRecovered).await?;
        self.flush_pending_application_data().await
    }

    fn transport_generation_is_current(&self, generation: TransportGeneration) -> bool {
        self.restart.generation_is_current(self.state.phase(), generation)
    }

    fn transport_event_is_relevant(&self, generation: TransportGeneration) -> bool {
        self.restart.event_is_relevant(generation)
    }
}

fn channel_close_is_terminal(phase: Phase) -> bool {
    matches!(phase, Phase::Negotiating | Phase::Connected | Phase::Reconnecting)
}

fn should_forward_local_candidate(
    phase: Phase,
    signaling_open: bool,
    current_generation: bool,
) -> bool {
    matches!(phase, Phase::Negotiating | Phase::Reconnecting)
        && signaling_open
        && current_generation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_channel_close_is_terminal_before_and_after_readiness() {
        for phase in [Phase::Negotiating, Phase::Connected, Phase::Reconnecting] {
            assert!(channel_close_is_terminal(phase));
        }
        assert!(!channel_close_is_terminal(Phase::Requesting));
        assert!(!channel_close_is_terminal(Phase::Incoming));
        assert!(!channel_close_is_terminal(Phase::Disconnecting));
    }

    #[test]
    fn local_candidates_are_only_forwarded_during_open_negotiation() {
        assert!(should_forward_local_candidate(Phase::Negotiating, true, true));
        assert!(!should_forward_local_candidate(Phase::Negotiating, false, true));
        assert!(!should_forward_local_candidate(Phase::Connected, false, true));
        assert!(!should_forward_local_candidate(Phase::Connected, true, true));
        assert!(should_forward_local_candidate(Phase::Reconnecting, true, true));
        assert!(!should_forward_local_candidate(Phase::Reconnecting, true, false));
    }
}

use std::sync::atomic::Ordering;

use webrtc::{
    data_channel::data_channel_state::RTCDataChannelState,
    dtls_transport::dtls_transport_state::RTCDtlsTransportState,
    ice_transport::{
        ice_candidate::RTCIceCandidateInit, ice_connection_state::RTCIceConnectionState,
        ice_gathering_state::RTCIceGatheringState, ice_transport_state::RTCIceTransportState,
    },
    peer_connection::{
        offer_answer_options::RTCOfferOptions, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, signaling_state::RTCSignalingState,
    },
};

use super::{super::share_epoch, Peer, ice_credentials::IceCredentials, rtc_operation};
use crate::peer_session::{Error, TransportGeneration};

impl Peer {
    pub(in crate::peer_session) async fn create_offer(&mut self) -> Result<String, Error> {
        let offer = rtc_operation(self.operation_timeout, self.pc.create_offer(None)).await?;
        let sdp = offer.sdp.clone();
        let credentials = IceCredentials::from_sdp(&sdp)?;
        self.set_local_description(TransportGeneration::INITIAL, offer, credentials).await?;
        Ok(sdp)
    }

    pub(in crate::peer_session) async fn apply_offer_and_create_answer(
        &mut self,
        sdp: String,
    ) -> Result<String, Error> {
        let remote_credentials = IceCredentials::from_sdp(&sdp)?;
        let offer =
            RTCSessionDescription::offer(sdp).map_err(|error| Error::WebRtc(error.to_string()))?;
        self.set_remote_description(TransportGeneration::INITIAL, offer, remote_credentials)
            .await?;
        let answer = rtc_operation(self.operation_timeout, self.pc.create_answer(None)).await?;
        let answer_sdp = answer.sdp.clone();
        let local_credentials = IceCredentials::from_sdp(&answer_sdp)?;
        self.set_local_description(TransportGeneration::INITIAL, answer, local_credentials).await?;
        Ok(answer_sdp)
    }

    pub(in crate::peer_session) async fn apply_answer(&mut self, sdp: String) -> Result<(), Error> {
        let credentials = IceCredentials::from_sdp(&sdp)?;
        let answer =
            RTCSessionDescription::answer(sdp).map_err(|error| Error::WebRtc(error.to_string()))?;
        self.set_remote_description(TransportGeneration::INITIAL, answer, credentials).await
    }

    pub(in crate::peer_session) async fn create_restart_offer(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<String, Error> {
        let previous_credentials = self
            .local_ice_credentials
            .clone()
            .ok_or_else(|| Error::WebRtc("initial local ICE credentials are unavailable".into()))?;
        self.begin_restart_generation(generation).await?;
        let offer = rtc_operation(
            self.operation_timeout,
            self.pc.create_offer(Some(RTCOfferOptions { ice_restart: true, ..Default::default() })),
        )
        .await?;
        let sdp = offer.sdp.clone();
        let credentials = IceCredentials::from_sdp(&sdp)?;
        credentials.require_rotation_from(&previous_credentials, "local")?;
        self.set_local_description(generation, offer, credentials).await?;
        Ok(sdp)
    }

    pub(in crate::peer_session) async fn apply_restart_offer_and_create_answer(
        &mut self,
        generation: TransportGeneration,
        sdp: String,
    ) -> Result<String, Error> {
        let remote_credentials = IceCredentials::from_sdp(&sdp)?;
        let previous_remote = self.remote_ice_credentials.clone().ok_or_else(|| {
            Error::WebRtc("initial remote ICE credentials are unavailable".into())
        })?;
        remote_credentials.require_rotation_from(&previous_remote, "remote")?;
        let previous_local = self
            .local_ice_credentials
            .clone()
            .ok_or_else(|| Error::WebRtc("initial local ICE credentials are unavailable".into()))?;
        self.begin_restart_generation(generation).await?;
        let offer =
            RTCSessionDescription::offer(sdp).map_err(|error| Error::WebRtc(error.to_string()))?;
        self.set_remote_description(generation, offer, remote_credentials).await?;
        let answer = rtc_operation(self.operation_timeout, self.pc.create_answer(None)).await?;
        let answer_sdp = answer.sdp.clone();
        let local_credentials = IceCredentials::from_sdp(&answer_sdp)?;
        local_credentials.require_rotation_from(&previous_local, "local")?;
        self.set_local_description(generation, answer, local_credentials).await?;
        Ok(answer_sdp)
    }

    pub(in crate::peer_session) async fn apply_restart_answer(
        &mut self,
        generation: TransportGeneration,
        sdp: String,
    ) -> Result<(), Error> {
        self.require_generation(generation)?;
        let credentials = IceCredentials::from_sdp(&sdp)?;
        let previous_credentials = self.remote_ice_credentials.as_ref().ok_or_else(|| {
            Error::WebRtc("initial remote ICE credentials are unavailable".into())
        })?;
        credentials.require_rotation_from(previous_credentials, "remote")?;
        let answer =
            RTCSessionDescription::answer(sdp).map_err(|error| Error::WebRtc(error.to_string()))?;
        self.set_remote_description(generation, answer, credentials).await
    }

    async fn begin_restart_generation(
        &mut self,
        generation: TransportGeneration,
    ) -> Result<(), Error> {
        if generation != self.generation.next()? {
            return Err(Error::Protocol(
                "WebRTC restart did not use the next transport generation".into(),
            ));
        }
        if self.pc.signaling_state() != RTCSignalingState::Stable {
            return Err(Error::WebRtc(
                "cannot restart ICE while WebRTC signaling is not stable".into(),
            ));
        }
        if self.pc.ice_gathering_state() == RTCIceGatheringState::Gathering {
            let mut gathering_complete = self.pc.gathering_complete_promise().await;
            tokio::time::timeout(self.operation_timeout, gathering_complete.recv())
                .await
                .map_err(|_| Error::OperationTimeout)?
                .ok_or_else(|| {
                    Error::WebRtc("ICE gathering completion notification was lost".into())
                })?;
        }

        self.generation = generation;
        self.callback_generation.store(generation.value(), Ordering::Release);
        self.local_description_generation = None;
        self.remote_description_generation = None;
        self.remote_candidates_received = 0;
        Ok(())
    }

    async fn set_local_description(
        &mut self,
        generation: TransportGeneration,
        description: RTCSessionDescription,
        credentials: IceCredentials,
    ) -> Result<(), Error> {
        self.require_generation(generation)?;
        let extension_id = self.validate_share_epoch_extension(&description.sdp)?;
        rtc_operation(self.operation_timeout, self.pc.set_local_description(description)).await?;
        self.share_epoch_extension_id = Some(extension_id);
        self.local_description_generation = Some(generation);
        self.local_ice_credentials = Some(credentials);
        Ok(())
    }

    async fn set_remote_description(
        &mut self,
        generation: TransportGeneration,
        description: RTCSessionDescription,
        credentials: IceCredentials,
    ) -> Result<(), Error> {
        self.require_generation(generation)?;
        self.network_policy.validate_remote_sdp(&description.sdp)?;
        let extension_id = self.validate_share_epoch_extension(&description.sdp)?;
        rtc_operation(self.operation_timeout, self.pc.set_remote_description(description)).await?;
        self.share_epoch_extension_id = Some(extension_id);
        self.remote_description_generation = Some(generation);
        self.remote_ice_credentials = Some(credentials);
        Ok(())
    }

    fn validate_share_epoch_extension(&self, sdp: &str) -> Result<u8, Error> {
        let extension_id = share_epoch::video_sdp_id(sdp)?;
        if self.share_epoch_extension_id.is_some_and(|negotiated| negotiated != extension_id) {
            return Err(Error::Protocol(
                "screen-share epoch RTP extension ID changed within the session".into(),
            ));
        }
        Ok(extension_id)
    }

    pub(in crate::peer_session) fn prepare_local_candidate(
        &self,
        generation: TransportGeneration,
        mut candidate: RTCIceCandidateInit,
    ) -> Result<RTCIceCandidateInit, Error> {
        self.require_generation(generation)?;
        if self.local_description_generation != Some(generation) {
            return Err(Error::Protocol(
                "local ICE candidate arrived before its description was installed".into(),
            ));
        }
        let expected = self
            .local_ice_credentials
            .as_ref()
            .ok_or_else(|| Error::WebRtc("local ICE credentials are unavailable".into()))?
            .username_fragment();
        if candidate.username_fragment.as_ref().is_some_and(|actual| actual != expected) {
            return Err(Error::Protocol(
                "local ICE candidate used the wrong username fragment".into(),
            ));
        }
        self.network_policy.validate_candidate(&candidate.candidate)?;
        candidate.username_fragment = Some(expected.to_owned());
        Ok(candidate)
    }

    pub(in crate::peer_session) async fn add_remote_candidate(
        &mut self,
        generation: TransportGeneration,
        candidate: RTCIceCandidateInit,
    ) -> Result<(), Error> {
        self.require_generation(generation)?;
        if self.remote_description_generation != Some(generation) {
            return Err(Error::Protocol(
                "ICE candidate arrived before its remote description".into(),
            ));
        }
        let expected = self
            .remote_ice_credentials
            .as_ref()
            .ok_or_else(|| Error::WebRtc("remote ICE credentials are unavailable".into()))?
            .username_fragment();
        if candidate.username_fragment.as_deref() != Some(expected) {
            return Err(Error::Protocol(
                "remote ICE candidate used the wrong username fragment".into(),
            ));
        }
        self.network_policy.validate_candidate(&candidate.candidate)?;
        if self.remote_candidates_received >= self.max_candidates_per_generation {
            return Err(Error::Protocol(
                "too many ICE candidates for one transport generation".into(),
            ));
        }
        self.remote_candidates_received += 1;
        rtc_operation(self.operation_timeout, self.pc.add_ice_candidate(candidate)).await
    }

    pub(in crate::peer_session) fn ice_connection_state(&self) -> RTCIceConnectionState {
        self.pc.ice_connection_state()
    }

    pub(in crate::peer_session) fn peer_connection_state(&self) -> RTCPeerConnectionState {
        self.pc.connection_state()
    }

    pub(in crate::peer_session) fn dtls_state(&self) -> RTCDtlsTransportState {
        self.pc.dtls_transport().state()
    }

    pub(in crate::peer_session) async fn transport_is_operational(
        &self,
        generation: TransportGeneration,
    ) -> Result<bool, Error> {
        self.require_generation(generation)?;
        if self.local_description_generation != Some(generation)
            || self.remote_description_generation != Some(generation)
            || !matches!(
                self.pc.ice_connection_state(),
                RTCIceConnectionState::Connected | RTCIceConnectionState::Completed
            )
            || self.pc.connection_state() != RTCPeerConnectionState::Connected
            || !matches!(
                self.pc.dtls_transport().ice_transport().state(),
                RTCIceTransportState::Connected | RTCIceTransportState::Completed
            )
            || self.pc.dtls_transport().state() != RTCDtlsTransportState::Connected
            || !self
                .control
                .as_ref()
                .is_some_and(|channel| channel.ready_state() == RTCDataChannelState::Open)
            || !self
                .messaging
                .as_ref()
                .is_some_and(|channel| channel.ready_state() == RTCDataChannelState::Open)
        {
            return Ok(false);
        }
        let pair = tokio::time::timeout(
            self.operation_timeout,
            self.pc.dtls_transport().ice_transport().get_selected_candidate_pair(),
        )
        .await
        .map_err(|_| Error::OperationTimeout)?;
        Ok(pair.is_some())
    }

    fn require_generation(&self, generation: TransportGeneration) -> Result<(), Error> {
        if generation != self.generation {
            return Err(Error::Protocol(
                "WebRTC operation used the wrong transport generation".into(),
            ));
        }
        Ok(())
    }
}

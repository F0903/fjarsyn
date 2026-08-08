use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate, peer_connection::RTCPeerConnection,
    rtp_transceiver::rtp_codec::RTPCodecType,
};

use super::{
    super::{Event, EventDispatcher},
    Peer,
};
use crate::peer_session::TransportGeneration;

impl Peer {
    pub(super) fn register_peer_callbacks(
        pc: &Arc<RTCPeerConnection>,
        events: EventDispatcher,
        callback_generation: Arc<AtomicU64>,
    ) {
        let candidate_events = events.clone();
        let candidate_generation = callback_generation.clone();
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let candidate_events = candidate_events.clone();
            let generation =
                TransportGeneration::from_value(candidate_generation.load(Ordering::Acquire));
            Box::pin(async move {
                let Some(candidate) = candidate else { return };
                match candidate.to_json() {
                    Ok(candidate) => {
                        candidate_events.dispatch(Event::LocalCandidate { generation, candidate })
                    }
                    Err(error) => candidate_events.dispatch(Event::Error(error.to_string())),
                }
            })
        }));

        let state_events = events.clone();
        let state_generation = callback_generation.clone();
        pc.on_peer_connection_state_change(Box::new(move |state| {
            let state_events = state_events.clone();
            let generation =
                TransportGeneration::from_value(state_generation.load(Ordering::Acquire));
            Box::pin(async move {
                state_events.dispatch(Event::PeerState { generation, state });
            })
        }));

        let ice_events = events.clone();
        let ice_generation = callback_generation.clone();
        pc.on_ice_connection_state_change(Box::new(move |state| {
            let ice_events = ice_events.clone();
            let generation =
                TransportGeneration::from_value(ice_generation.load(Ordering::Acquire));
            Box::pin(async move {
                ice_events.dispatch(Event::IceState { generation, state });
            })
        }));

        let dtls_events = events.clone();
        let dtls_generation = callback_generation.clone();
        pc.dtls_transport().on_state_change(Box::new(move |state| {
            let dtls_events = dtls_events.clone();
            let generation =
                TransportGeneration::from_value(dtls_generation.load(Ordering::Acquire));
            Box::pin(async move {
                dtls_events.dispatch(Event::DtlsState { generation, state });
            })
        }));

        let channel_events = events.clone();
        pc.on_data_channel(Box::new(move |channel| {
            let channel_events = channel_events.clone();
            Box::pin(async move {
                channel_events.dispatch(Event::DataChannel(channel));
            })
        }));

        pc.on_track(Box::new(move |track, _receiver, transceiver| {
            let events = events.clone();
            Box::pin(async move {
                if track.kind() == RTPCodecType::Video {
                    events.dispatch(Event::RemoteTrack(track, transceiver));
                } else {
                    events
                        .dispatch(Event::ProtocolError("unexpected non-video media track".into()));
                }
            })
        }));
    }
}

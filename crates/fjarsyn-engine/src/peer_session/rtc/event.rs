use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::{mpsc, watch};
use webrtc::{
    data_channel::RTCDataChannel,
    dtls_transport::dtls_transport_state::RTCDtlsTransportState,
    ice_transport::{
        ice_candidate::RTCIceCandidateInit, ice_connection_state::RTCIceConnectionState,
    },
    peer_connection::peer_connection_state::RTCPeerConnectionState,
    rtp_transceiver::RTCRtpTransceiver,
    track::track_remote::TrackRemote,
};

use super::ChannelKind;
use crate::peer_session::TransportGeneration;

pub(in crate::peer_session) enum Event {
    LocalCandidate { generation: TransportGeneration, candidate: RTCIceCandidateInit },
    IceState { generation: TransportGeneration, state: RTCIceConnectionState },
    DtlsState { generation: TransportGeneration, state: RTCDtlsTransportState },
    PeerState { generation: TransportGeneration, state: RTCPeerConnectionState },
    DataChannel(Arc<RTCDataChannel>),
    ChannelOpen(ChannelKind),
    ChannelClosed(ChannelKind),
    ChannelMessage(ChannelKind, Bytes),
    RemoteTrack(Arc<TrackRemote>, Arc<RTCRtpTransceiver>),
    Error(String),
    ProtocolError(String),
}

#[derive(Clone)]
pub(super) struct EventDispatcher {
    tx: mpsc::Sender<Event>,
    fatal_tx: watch::Sender<Option<String>>,
}

impl EventDispatcher {
    pub(super) fn new(tx: mpsc::Sender<Event>, fatal_tx: watch::Sender<Option<String>>) -> Self {
        Self { tx, fatal_tx }
    }

    pub(super) fn dispatch(&self, event: Event) {
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.fatal_tx.send_replace(Some("WebRTC event queue overflowed".into()));
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

    use super::*;

    #[test]
    fn queue_overflow_uses_the_nonblocking_fatal_path() {
        let (tx, _rx) = mpsc::channel(1);
        let (fatal_tx, fatal_rx) = watch::channel(None);
        let events = EventDispatcher::new(tx, fatal_tx);
        events.dispatch(Event::PeerState {
            generation: TransportGeneration::INITIAL,
            state: RTCPeerConnectionState::New,
        });
        events.dispatch(Event::PeerState {
            generation: TransportGeneration::INITIAL,
            state: RTCPeerConnectionState::Connecting,
        });
        assert_eq!(fatal_rx.borrow().as_deref(), Some("WebRTC event queue overflowed"));
    }
}

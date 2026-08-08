use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;

use crate::peer_session::TransportGeneration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(in crate::peer_session) enum NegotiationSignal {
    EndpointHello { challenge: Uuid },
    EndpointProof { challenge: Uuid },
    Request {},
    Restart { generation: TransportGeneration },
    RestartAck { generation: TransportGeneration },
    Accept {},
    Offer { generation: TransportGeneration, sdp: String },
    Answer { generation: TransportGeneration, sdp: String },
    IceCandidate { generation: TransportGeneration, candidate: RTCIceCandidateInit },
    Ready { generation: TransportGeneration },
    ReadyAck { generation: TransportGeneration },
    Reject { reason: String },
    Cancel {},
}

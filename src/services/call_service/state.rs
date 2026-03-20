use crate::{networking::webrtc::WebRTCEvent, ui::message::CallTarget};

#[derive(Debug, Clone)]
pub enum CallEvent {
    IncomingCall { peer_id: String },
    CallConnected,
    CallEnded,
    RemoteStreamStarted,
    RemoteStreamEnded,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum CallState {
    #[default]
    Idle,
    Dialing {
        target: CallTarget,
    },
    IncomingCall {
        peer_id: String,
    },
    InCall {
        peer_id: Option<String>,
    },
}

impl CallState {
    pub fn apply_event(&mut self, event: WebRTCEvent) -> Option<CallEvent> {
        match event {
            WebRTCEvent::IncomingCall(peer_id) => {
                if matches!(*self, CallState::Idle) {
                    *self = CallState::IncomingCall { peer_id: peer_id.clone() };
                    Some(CallEvent::IncomingCall { peer_id })
                } else {
                    None
                }
            }
            WebRTCEvent::Connected => {
                let peer_id = match self {
                    CallState::Dialing { .. } => None,
                    CallState::IncomingCall { peer_id } => Some(peer_id.clone()),
                    CallState::InCall { peer_id } => peer_id.clone(),
                    _ => None,
                };
                *self = CallState::InCall { peer_id };
                Some(CallEvent::CallConnected)
            }
            WebRTCEvent::Disconnected => {
                *self = CallState::Idle;
                Some(CallEvent::CallEnded)
            }
            WebRTCEvent::RemoteStreamStarted => match self {
                CallState::InCall { .. } => Some(CallEvent::RemoteStreamStarted),
                _ => None,
            },
            WebRTCEvent::RemoteStreamEnded => match self {
                CallState::InCall { .. } => Some(CallEvent::RemoteStreamEnded),
                _ => None,
            },
        }
    }
}

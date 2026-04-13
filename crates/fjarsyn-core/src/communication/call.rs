#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
    PeerId(String),
    Address(String),
    ContactId(i64),
}

#[derive(Debug, Clone)]
pub enum CallEvent {
    IncomingCall { peer_id: String },
    CallConnected,
    CallEnded,
    RemoteStreamStarted,
    RemoteStreamEnded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTransportEvent {
    Connected(Option<String>),
    Disconnected,
    IncomingCall(String),
    RemoteStreamStarted,
    RemoteStreamEnded,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum CallState {
    #[default]
    Idle,
    Dialing {
        target: CallTarget,
        peer_id: Option<String>,
    },
    IncomingCall {
        peer_id: String,
    },
    InCall {
        peer_id: Option<String>,
    },
}

impl CallState {
    pub fn apply_event(&mut self, event: CallTransportEvent) -> Option<CallEvent> {
        match event {
            CallTransportEvent::IncomingCall(peer_id) => {
                if matches!(*self, CallState::Idle) {
                    *self = CallState::IncomingCall { peer_id: peer_id.clone() };
                    Some(CallEvent::IncomingCall { peer_id })
                } else {
                    None
                }
            }
            CallTransportEvent::Connected(remote_peer_id) => {
                let peer_id = match self {
                    CallState::Dialing { peer_id, .. } => peer_id.clone(),
                    CallState::IncomingCall { peer_id } => Some(peer_id.clone()),
                    CallState::InCall { peer_id } => peer_id.clone(),
                    CallState::Idle => remote_peer_id,
                };
                *self = CallState::InCall { peer_id };
                Some(CallEvent::CallConnected)
            }
            CallTransportEvent::Disconnected => {
                *self = CallState::Idle;
                Some(CallEvent::CallEnded)
            }
            CallTransportEvent::RemoteStreamStarted => match self {
                CallState::InCall { .. } => Some(CallEvent::RemoteStreamStarted),
                _ => None,
            },
            CallTransportEvent::RemoteStreamEnded => match self {
                CallState::InCall { .. } => Some(CallEvent::RemoteStreamEnded),
                _ => None,
            },
        }
    }
}

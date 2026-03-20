use super::{CallEvent, CallState};
use crate::{networking::webrtc::WebRTCEvent, ui::message::CallTarget};

#[test]
fn connected_event_preserves_known_peer_id() {
    let mut state = CallState::InCall { peer_id: Some("peer-123".into()) };

    let event = state.apply_event(WebRTCEvent::Connected);

    assert!(matches!(event, Some(CallEvent::CallConnected)));
    assert_eq!(state, CallState::InCall { peer_id: Some("peer-123".into()) });
}

#[test]
fn disconnected_event_returns_to_idle() {
    let mut state = CallState::Dialing { target: CallTarget::PeerId("peer-123".into()) };

    let event = state.apply_event(WebRTCEvent::Disconnected);

    assert!(matches!(event, Some(CallEvent::CallEnded)));
    assert_eq!(state, CallState::Idle);
}

#[test]
fn remote_stream_events_do_not_change_call_state() {
    let mut state = CallState::InCall { peer_id: Some("peer-123".into()) };

    let started_event = state.apply_event(WebRTCEvent::RemoteStreamStarted);
    let ended_event = state.apply_event(WebRTCEvent::RemoteStreamEnded);

    assert!(matches!(started_event, Some(CallEvent::RemoteStreamStarted)));
    assert!(matches!(ended_event, Some(CallEvent::RemoteStreamEnded)));
    assert_eq!(state, CallState::InCall { peer_id: Some("peer-123".into()) });
}

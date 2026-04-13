pub use crate::communication::call::{CallEvent, CallState, CallTarget, CallTransportEvent};
use crate::networking::webrtc::WebRTCEvent;

pub fn map_webrtc_event(event: WebRTCEvent) -> CallTransportEvent {
    match event {
        WebRTCEvent::Connected => CallTransportEvent::Connected(None),
        WebRTCEvent::Disconnected => CallTransportEvent::Disconnected,
        WebRTCEvent::IncomingCall(peer_id) => CallTransportEvent::IncomingCall(peer_id),
        WebRTCEvent::RemoteStreamStarted => CallTransportEvent::RemoteStreamStarted,
        WebRTCEvent::RemoteStreamEnded => CallTransportEvent::RemoteStreamEnded,
    }
}

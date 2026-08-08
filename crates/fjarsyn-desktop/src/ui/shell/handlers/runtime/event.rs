use fjarsyn_engine::{messaging, peer_session, screen_share};

use crate::ui::{runtime, shell::Fjarsyn};

pub(super) fn apply(app: &mut Fjarsyn, event: runtime::Event) {
    match event {
        runtime::Event::Presence(snapshot) => app.state.presence = snapshot,
        runtime::Event::Sessions(snapshot) => app.state.sessions = snapshot,
        runtime::Event::SessionChange(event) => apply_session_change(app, event),
        runtime::Event::Messaging(snapshot) => app.state.messaging = snapshot,
        runtime::Event::MessagingChange(event) => {
            if let messaging::Event::IncomingMessage { peer_id, .. } = event {
                app.state
                    .notify_info(format!("New message from {}.", app.state.display_name(&peer_id)));
            }
        }
        runtime::Event::ScreenShareSnapshotReady(update) => {
            if let Some(snapshot) = update.take_latest() {
                app.state.screen_share = snapshot;
            }
        }
        runtime::Event::ScreenShareChange(event) => apply_screen_share_event(app, event),
    }
}

fn apply_session_change(app: &mut Fjarsyn, event: peer_session::Event) {
    match event {
        peer_session::Event::IncomingRequest { peer_id, .. } => {
            app.state.notify_info(format!(
                "Connection request from {}.",
                app.state.display_name(&peer_id)
            ));
        }
        peer_session::Event::Connected { peer_id, .. } => {
            app.state.notify_success(format!("Connected to {}.", app.state.display_name(&peer_id)));
        }
        peer_session::Event::Closed { peer_id, reason, .. }
            if !matches!(reason, peer_session::CloseReason::LocalDisconnect) =>
        {
            app.state.notify_info(format!(
                "Session with {} closed: {}",
                app.state.display_name(&peer_id),
                close_reason(&reason),
            ));
        }
        _ => {}
    }
}

fn apply_screen_share_event(app: &mut Fjarsyn, event: screen_share::Event) {
    match event {
        screen_share::Event::LocalFailed { reason, .. }
        | screen_share::Event::RemoteFailed { reason, .. } => app.state.notify_error(reason),
        screen_share::Event::CodecRestartRequired { .. } => {}
    }
}

fn close_reason(reason: &peer_session::CloseReason) -> String {
    match reason {
        peer_session::CloseReason::LocalDisconnect => "disconnected".into(),
        peer_session::CloseReason::RemoteDisconnect => "the contact disconnected".into(),
        peer_session::CloseReason::Rejected { reason } => format!("rejected ({reason})"),
        peer_session::CloseReason::Cancelled => "cancelled".into(),
        peer_session::CloseReason::SignalingLost => "signaling was lost".into(),
        peer_session::CloseReason::ConnectionFailed { reason } => reason.clone(),
        peer_session::CloseReason::ProtocolViolation { reason } => {
            format!("protocol violation ({reason})")
        }
        peer_session::CloseReason::TrustRevoked => "the contact's trusted identity changed".into(),
        peer_session::CloseReason::ServiceShutdown => "Fjarsyn is shutting down".into(),
    }
}

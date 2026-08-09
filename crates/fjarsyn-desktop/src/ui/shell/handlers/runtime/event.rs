use fjarsyn_engine::peer_session;

use crate::ui::{
    runtime,
    shell::{Fjarsyn, Lifecycle},
};

pub(super) fn apply_engine_state(app: &mut Fjarsyn, state: runtime::EngineState) {
    app.state.presence = state.presence;
    app.state.sessions = state.sessions;
    app.state.messaging = state.messaging;
    app.state.screen_share = state.screen_share;
}

pub(super) fn apply_notice(app: &mut Fjarsyn, notice: runtime::EngineNotice) {
    match notice {
        runtime::EngineNotice::IncomingRequest { peer_id } => {
            app.state.notify_info(format!(
                "Connection request from {}.",
                app.state.display_name(&peer_id)
            ));
        }
        runtime::EngineNotice::Connected { peer_id } => {
            app.state.notify_success(format!("Connected to {}.", app.state.display_name(&peer_id)));
        }
        runtime::EngineNotice::Closed { peer_id, reason } => {
            app.state.notify_info(format!(
                "Session with {} closed: {}",
                app.state.display_name(&peer_id),
                close_reason(&reason),
            ));
        }
        runtime::EngineNotice::IncomingMessage { peer_id } => {
            app.state.notify_info(format!("New message from {}.", app.state.display_name(&peer_id)))
        }
        runtime::EngineNotice::ScreenShareFailed { reason } => {
            app.state.notify_error(reason);
        }
    }
}

pub(super) fn apply_failure(app: &mut Fjarsyn, failure: runtime::EngineAdapterFailure) {
    let reason = failure.to_string();
    app.state.notify_error(format!("{reason}. Restart Fjarsyn to restore live updates."));
    app.state.lifecycle = Lifecycle::Degraded(reason);
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

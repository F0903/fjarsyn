use super::{AppCommands, notify_error};
use crate::{
    app::{AppCommand, AppState, resolve_call_target_hint},
    communication::call::CallTarget,
};

#[derive(Debug, Clone)]
pub enum CallAction {
    AcceptRequested,
    AcceptFailed { error: String, peer_id: Option<String> },
    DeclineRequested,
    DeclineFailed { error: String, peer_id: Option<String> },
    StartRequested(CallTarget),
    StartFailed(String),
}

pub fn execute_call_action(state: &mut AppState, action: CallAction) -> AppCommands {
    match action {
        CallAction::AcceptRequested => {
            if !state.can_control_calls() {
                return smallvec::smallvec![notify_error(call_unavailable_message(state))];
            }

            state.session.incoming_call_id = None;
            state.session.incoming_call_timeout = None;
            state.session.call_connected = false;
            smallvec::smallvec![AppCommand::AcceptCall]
        }
        CallAction::AcceptFailed { error, peer_id } => {
            if let Some(peer_id) = peer_id {
                restore_incoming_call(state, peer_id);
            }
            smallvec::smallvec![notify_error(format!("Failed to accept incoming call: {}", error))]
        }
        CallAction::DeclineRequested => {
            if !state.can_control_calls() {
                return smallvec::smallvec![notify_error(call_unavailable_message(state))];
            }

            clear_session(state);
            smallvec::smallvec![AppCommand::DeclineCall]
        }
        CallAction::DeclineFailed { error, peer_id } => {
            if let Some(peer_id) = peer_id {
                restore_incoming_call(state, peer_id);
            }
            smallvec::smallvec![notify_error(format!("Failed to decline incoming call: {}", error))]
        }
        CallAction::StartRequested(target) => {
            if !state.can_control_calls() {
                return smallvec::smallvec![notify_error(call_unavailable_message(state))];
            }

            state.session.call_connected = false;

            let (target_id, target_label) = resolve_call_target_hint(
                &target,
                &state.contacts.contacts,
                &state.networking.discovered_peers,
            );

            state.session.target_id = target_id;
            state.session.target_label = target_label;
            state.session.incoming_call_id = None;
            state.session.incoming_call_timeout = None;

            smallvec::smallvec![AppCommand::StartCall { target }]
        }
        CallAction::StartFailed(error) => {
            clear_session(state);
            smallvec::smallvec![notify_error(error)]
        }
    }
}

fn call_unavailable_message(state: &AppState) -> &'static str {
    if !state.accepts_user_requests() {
        "Calls are unavailable while the app is shutting down."
    } else {
        "Call service is unavailable until startup completes."
    }
}

pub(crate) fn restore_incoming_call(state: &mut AppState, peer_id: String) {
    state.session.target_id = Some(peer_id.clone());
    state.session.target_label = state
        .networking
        .discovered_peers
        .iter()
        .find(|peer| peer.id == peer_id)
        .map(|peer| peer.instance_name.trim().to_string())
        .filter(|name| !name.is_empty());
    state.session.incoming_call_id = Some(peer_id);
    state.session.incoming_call_timeout =
        Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
    state.session.call_connected = false;
}

pub(crate) fn clear_session(state: &mut AppState) {
    state.session.target_id = None;
    state.session.target_label = None;
    state.session.incoming_call_id = None;
    state.session.incoming_call_timeout = None;
    state.session.call_connected = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::NotificationLevel, executors::test_support::state};

    #[test]
    fn starting_call_requires_ready_call_service() {
        let mut state = state();

        let commands = execute_call_action(
            &mut state,
            CallAction::StartRequested(CallTarget::PeerId("peer-a".into())),
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::Notify { level: NotificationLevel::Error, .. }
        )));
        assert!(state.session.target_id.is_none());
    }
}

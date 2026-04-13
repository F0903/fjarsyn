use std::sync::Arc;

use super::{AppCommands, notify_info};
use crate::app::{AppCommand, AppLifecycle, AppState, ServicesState, request_shutdown};

#[derive(Debug, Clone)]
pub enum LifecycleAction {
    ShutdownRequested,
    RetryStartupRequested,
}

pub fn execute_lifecycle_action(state: &mut AppState, action: LifecycleAction) -> AppCommands {
    match action {
        LifecycleAction::ShutdownRequested => {
            request_shutdown(state);
            AppCommands::new()
        }
        LifecycleAction::RetryStartupRequested => {
            if matches!(state.lifecycle, AppLifecycle::ShuttingDown) {
                return AppCommands::new();
            }

            reset_for_startup_retry(state);
            smallvec::smallvec![notify_info("Retrying startup..."), AppCommand::RetryStartup]
        }
    }
}

fn reset_for_startup_retry(state: &mut AppState) {
    state.networking.local_peer_id = None;
    state.networking.discovered_peers.clear();
    state.networking.recent_peers.clear();

    state.session.target_id = None;
    state.session.target_label = None;
    state.session.incoming_call_id = None;
    state.session.incoming_call_timeout = None;
    state.session.call_connected = false;

    state.messaging.summaries = Arc::new(Vec::new());
    state.messaging.active_peer_id = None;
    state.messaging.active_messages = Arc::new(Vec::new());
    state.messaging.revision = state.messaging.revision.wrapping_add(1);

    state.contacts.contacts = Arc::new(Vec::new());
    state.services = ServicesState::default();
    state.lifecycle = AppLifecycle::Bootstrapping;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{AppLifecycle, ServicePhase, ServicesState, recompute_lifecycle},
        executors::{
            AppEvent, ContactsAction, MessagingAction, ServiceAction, execute_app_event,
            execute_messaging_action, execute_service_action, test_support::state,
        },
        networking::discovery::PeerInfo,
    };

    #[test]
    fn lifecycle_becomes_degraded_when_optional_services_fail() {
        let mut state = state();

        execute_service_action(
            &mut state,
            ServiceAction::CallServiceReady {
                local_peer_id: "runtime-peer".into(),
                signaling_port: 9000,
                persist_local_peer_id: false,
            },
        );
        execute_service_action(&mut state, ServiceAction::DatabaseReady);
        execute_service_action(
            &mut state,
            ServiceAction::DiscoveryServiceInitFailed("offline".into()),
        );
        execute_messaging_action(&mut state, MessagingAction::ServiceInitFailed("disabled".into()));
        recompute_lifecycle(&mut state);

        assert_eq!(state.lifecycle, AppLifecycle::Degraded);
    }

    #[test]
    fn lifecycle_becomes_failed_when_call_service_fails() {
        let mut state = state();

        execute_service_action(
            &mut state,
            ServiceAction::CallServiceInitFailed("bind failed".into()),
        );
        recompute_lifecycle(&mut state);

        assert_eq!(state.lifecycle, AppLifecycle::Failed);
        assert_eq!(state.services.discovery, ServicePhase::Failed);
        assert_eq!(state.services.messaging, ServicePhase::Failed);
    }

    #[test]
    fn shutdown_requested_ignores_follow_up_app_events() {
        let mut state = state();

        let commands =
            execute_app_event(&mut state, AppEvent::Lifecycle(LifecycleAction::ShutdownRequested));

        assert!(commands.is_empty());
        assert_eq!(state.lifecycle, AppLifecycle::ShuttingDown);

        let commands =
            execute_app_event(&mut state, AppEvent::Contacts(ContactsAction::LoadRequested));

        assert!(commands.is_empty());
    }

    #[test]
    fn retry_startup_resets_runtime_state_and_emits_retry_command() {
        let mut state = state();
        state.lifecycle = AppLifecycle::Failed;
        state.services.call = ServicePhase::Failed;
        state.services.discovery = ServicePhase::Failed;
        state.services.messaging = ServicePhase::Failed;
        state.networking.local_peer_id = Some("runtime-peer".into());
        state.networking.discovered_peers.push(PeerInfo {
            id: "peer-b".into(),
            instance_name: "fjarsyn-peer-b".into(),
            host_name: "fjarsyn-peer-b.local.".into(),
            addresses: vec![std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)],
            port: 9001,
        });
        state.messaging.active_peer_id = Some("peer-b".into());

        let commands = execute_app_event(
            &mut state,
            AppEvent::Lifecycle(LifecycleAction::RetryStartupRequested),
        );

        assert_eq!(state.lifecycle, AppLifecycle::Bootstrapping);
        assert_eq!(state.services, ServicesState::default());
        assert!(state.networking.local_peer_id.is_none());
        assert!(state.networking.discovered_peers.is_empty());
        assert!(state.messaging.active_peer_id.is_none());
        assert!(commands.iter().any(|command| matches!(command, AppCommand::RetryStartup)));
    }
}

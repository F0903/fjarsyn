use super::{
    AppCommands,
    call::{clear_session, restore_incoming_call},
    notify_error, notify_info,
};
use crate::{
    app::{AppCommand, AppState, Route, ServicePhase, update_recent_peer},
    communication::call::CallEvent,
    networking::discovery::{DiscoveryEvent, PeerInfo},
};

#[derive(Debug, Clone)]
pub enum ServiceAction {
    CallServiceReady { local_peer_id: String, signaling_port: u16, persist_local_peer_id: bool },
    CallServiceInitFailed(String),
    DiscoveryServiceReady,
    DiscoveryServiceInitFailed(String),
    DatabaseReady,
    DatabaseInitFailed(String),
    CallEvent(CallEvent),
    DiscoveryEvent(DiscoveryEvent),
    PeerFound(PeerInfo),
}

pub fn execute_service_action(state: &mut AppState, action: ServiceAction) -> AppCommands {
    match action {
        ServiceAction::CallServiceReady {
            local_peer_id,
            signaling_port,
            persist_local_peer_id,
        } => {
            state.services.call = ServicePhase::Ready;
            state.networking.local_peer_id = Some(local_peer_id.clone());

            let mut commands = AppCommands::new();
            if persist_local_peer_id {
                state.config.identity.peer_id = Some(local_peer_id.clone());
                commands.push(AppCommand::SaveConfig {
                    success_message: None,
                    error_message: "Failed to save peer ID".into(),
                });
            }

            commands.push(AppCommand::InitializeDiscovery { local_peer_id, signaling_port });
            if should_initialize_messaging(state) {
                commands.push(AppCommand::InitializeMessaging);
            }
            commands
        }
        ServiceAction::CallServiceInitFailed(err) => {
            state.services.call = ServicePhase::Failed;
            state.services.discovery = ServicePhase::Failed;
            state.services.messaging = ServicePhase::Failed;
            smallvec::smallvec![notify_error(format!("Call service failed to initialize: {}", err))]
        }
        ServiceAction::DiscoveryServiceReady => {
            state.services.discovery = ServicePhase::Ready;
            AppCommands::new()
        }
        ServiceAction::DiscoveryServiceInitFailed(err) => {
            state.services.discovery = ServicePhase::Failed;
            smallvec::smallvec![notify_error(format!(
                "Discovery service failed to initialize: {}",
                err
            ))]
        }
        ServiceAction::DatabaseReady => {
            state.services.database = ServicePhase::Ready;

            let mut commands = smallvec::smallvec![AppCommand::LoadContacts];
            if should_initialize_messaging(state) {
                commands.push(AppCommand::InitializeMessaging);
            }
            commands
        }
        ServiceAction::DatabaseInitFailed(err) => {
            state.services.database = ServicePhase::Failed;
            state.services.messaging = ServicePhase::Failed;
            smallvec::smallvec![notify_error(format!("DB Failed: {}", err))]
        }
        ServiceAction::CallEvent(event) => execute_call_event(state, event),
        ServiceAction::DiscoveryEvent(event) => {
            apply_discovery_event(state, event);
            AppCommands::new()
        }
        ServiceAction::PeerFound(peer) => {
            maybe_upsert_peer(state, peer);
            AppCommands::new()
        }
    }
}

fn should_initialize_messaging(state: &AppState) -> bool {
    state.services.database == ServicePhase::Ready
        && state.services.call == ServicePhase::Ready
        && state.services.messaging == ServicePhase::Pending
}

fn execute_call_event(state: &mut AppState, event: CallEvent) -> AppCommands {
    match event {
        CallEvent::IncomingCall { peer_id } => {
            restore_incoming_call(state, peer_id);
            AppCommands::new()
        }
        CallEvent::CallConnected => {
            state.session.incoming_call_id = None;
            state.session.incoming_call_timeout = None;
            state.session.call_connected = true;
            update_recent_peer(
                &mut state.networking.recent_peers,
                &state.networking.discovered_peers,
                state.session.target_id.as_deref(),
            );
            smallvec::smallvec![AppCommand::Navigate(Route::Call)]
        }
        CallEvent::CallEnded => {
            let had_target = state.session.target_id.is_some();
            clear_session(state);

            if had_target {
                smallvec::smallvec![notify_info("Call ended.")]
            } else {
                AppCommands::new()
            }
        }
        CallEvent::RemoteStreamStarted | CallEvent::RemoteStreamEnded => AppCommands::new(),
    }
}

fn apply_discovery_event(state: &mut AppState, event: DiscoveryEvent) {
    match event {
        DiscoveryEvent::PeerFound(peer) => maybe_upsert_peer(state, peer),
        DiscoveryEvent::PeerRemoved(fullname) => {
            state.networking.discovered_peers.retain(|peer| peer.instance_name != fullname);
        }
    }
}

fn maybe_upsert_peer(state: &mut AppState, peer: PeerInfo) {
    let local_peer_id =
        state.networking.local_peer_id.as_deref().or(state.config.identity.peer_id.as_deref());

    if local_peer_id.is_some_and(|local_id| local_id == peer.id) {
        return;
    }

    if let Some(existing) = state.networking.discovered_peers.iter_mut().find(|p| p.id == peer.id) {
        existing.update(peer);
    } else {
        state.networking.discovered_peers.push(peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::ServicePhase, executors::test_support::state};

    #[test]
    fn database_ready_requests_contacts() {
        let mut state = state();

        let commands = execute_service_action(&mut state, ServiceAction::DatabaseReady);

        assert_eq!(state.services.database, ServicePhase::Ready);
        assert!(commands.iter().any(|command| matches!(command, AppCommand::LoadContacts)));
    }

    #[test]
    fn call_service_ready_tracks_runtime_local_peer_id_without_persisting_when_disabled() {
        let mut state = state();
        state.config.identity.peer_id = Some("persisted-peer".into());

        let commands = execute_service_action(
            &mut state,
            ServiceAction::CallServiceReady {
                local_peer_id: "runtime-peer".into(),
                signaling_port: 9000,
                persist_local_peer_id: false,
            },
        );

        assert_eq!(state.networking.local_peer_id.as_deref(), Some("runtime-peer"));
        assert_eq!(state.config.identity.peer_id.as_deref(), Some("persisted-peer"));
        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::InitializeDiscovery { local_peer_id, signaling_port }
            if local_peer_id == "runtime-peer" && *signaling_port == 9000
        )));
        assert!(!commands.iter().any(|command| matches!(command, AppCommand::SaveConfig { .. })));
    }

    #[test]
    fn peer_found_ignores_runtime_local_peer_id_even_when_config_is_stale() {
        let mut state = state();
        state.config.identity.peer_id = Some("persisted-peer".into());
        state.networking.local_peer_id = Some("runtime-peer".into());

        let commands = execute_service_action(
            &mut state,
            ServiceAction::PeerFound(PeerInfo {
                id: "runtime-peer".into(),
                instance_name: "fjarsyn-runtime-peer".into(),
                host_name: "fjarsyn-runtime-peer.local.".into(),
                addresses: vec![std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)],
                port: 9000,
            }),
        );

        assert!(commands.is_empty());
        assert!(state.networking.discovered_peers.is_empty());
    }
}

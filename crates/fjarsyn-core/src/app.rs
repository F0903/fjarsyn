use std::{collections::VecDeque, net::SocketAddr, sync::Arc};

use crate::{
    call::{CallEvent, CallTarget},
    config::Config,
    discovery::{DiscoveryEvent, PeerInfo},
    messaging::{ConversationMessage, ConversationSummary, MessagingEvent},
    navigation::Route,
    services::contacts_service::Contact,
    text,
};

pub struct NetworkingState {
    pub local_peer_id: Option<String>,
    pub discovered_peers: Vec<PeerInfo>,
    pub recent_peers: Vec<PeerInfo>,
}

pub struct SessionState {
    pub target_id: Option<String>,
    pub target_label: Option<String>,
    pub incoming_call_id: Option<String>,
    pub incoming_call_timeout: Option<std::time::Instant>,
    pub call_connected: bool,
}

pub struct MessagingState {
    pub summaries: Arc<Vec<ConversationSummary>>,
    pub active_peer_id: Option<String>,
    pub active_messages: Arc<Vec<ConversationMessage>>,
    pub revision: u64,
}

pub struct ContactsState {
    pub contacts: Arc<Vec<Contact>>,
}

#[derive(Default)]
pub struct ServicesState {
    pub database_ready: bool,
    pub call_ready: bool,
    pub discovery_ready: bool,
    pub messaging_ready: bool,
}

pub struct AppState {
    pub networking: NetworkingState,
    pub session: SessionState,
    pub messaging: MessagingState,
    pub contacts: ContactsState,
    pub config: Config,
    pub services: ServicesState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Error,
    Info,
    Success,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    Notify { level: NotificationLevel, message: String },
    SaveConfig,
    Navigate(Route),
    LoadContacts,
    SaveContact { peer_id: String, name: String, address: Option<String> },
    DeleteContact { id: i64 },
    UpdateContactAddress { id: i64, peer_id: String, name: String, address: String },
    AcceptCall,
    DeclineCall,
    StartCall { target: CallTarget },
    SendMessage { peer_id: String, address: SocketAddr, body: String },
    InitializeDiscovery { local_peer_id: String, signaling_port: u16 },
    InitializeMessaging,
    RefreshActiveConversation,
    ClearMessageDraft(String),
}

#[derive(Debug, Clone)]
pub enum ContactsAction {
    LoadRequested,
    SaveRequested { peer_id: String, name: String, address: Option<String> },
    DeleteRequested(i64),
    UpdateAddressRequested { id: i64, new_address: String },
    Loaded(Result<Arc<Vec<Contact>>, String>),
    Saved(Result<Arc<Vec<Contact>>, String>),
    Deleted(Result<Arc<Vec<Contact>>, String>),
    Updated(Result<Arc<Vec<Contact>>, String>),
}

#[derive(Debug, Clone)]
pub enum MessagingAction {
    ServiceReady { summaries: Arc<Vec<ConversationSummary>> },
    ServiceInitFailed(String),
    SendRequested { peer_id: String, address: SocketAddr, body: String },
    SendCompleted(Result<String, String>),
    Event { event: MessagingEvent, summaries: Arc<Vec<ConversationSummary>> },
    ActiveConversationSelected(Option<String>),
    ActiveConversationLoaded(Arc<Vec<ConversationMessage>>),
}

#[derive(Debug, Clone)]
pub enum CallAction {
    AcceptRequested,
    DeclineRequested,
    StartRequested(CallTarget),
}

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

pub fn resolve_selected_peer_id(
    summaries: &[ConversationSummary],
    selected_peer_id: Option<String>,
) -> Option<String> {
    selected_peer_id.or_else(|| summaries.first().map(|summary| summary.peer_id.clone()))
}

pub fn resolve_call_target_hint(
    target: &CallTarget,
    contacts: &[Contact],
    discovered: &[PeerInfo],
) -> (Option<String>, Option<String>) {
    match target {
        CallTarget::PeerId(id) => {
            let label = discovered
                .iter()
                .find(|peer| peer.id == *id)
                .and_then(|peer| non_empty(peer.instance_name.clone()));
            (Some(id.clone()), label)
        }
        CallTarget::Address(addr) => (None, Some(addr.clone())),
        CallTarget::ContactId(id) => contacts
            .iter()
            .find(|contact| contact.id == *id)
            .map(|contact| {
                (
                    Some(contact.peer_id.clone()),
                    non_empty(contact.name.clone()).or_else(|| contact.address.clone()),
                )
            })
            .unwrap_or((None, None)),
    }
}

pub fn peer_label(contacts: &[Contact], discovered: &[PeerInfo], peer_id: &str) -> String {
    if let Some(contact) = contacts.iter().find(|contact| contact.peer_id == peer_id) {
        return contact.name.clone();
    }

    discovered
        .iter()
        .find(|peer| peer.id == peer_id)
        .map(|peer| peer.instance_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| text::truncate(peer_id, 12).to_string())
}

pub fn peer_display_name(
    contacts: &[Contact],
    discovered: &[PeerInfo],
    peer_id: &str,
    max_chars: usize,
) -> String {
    if let Some(contact) = contacts.iter().find(|contact| contact.peer_id == peer_id) {
        return text::truncate_with_ellipsis(contact.name.trim(), max_chars);
    }

    discovered
        .iter()
        .find(|peer| peer.id == peer_id)
        .map(|peer| peer.instance_name.trim().to_string())
        .filter(|name| !name.is_empty())
        .map(|name| text::truncate_with_ellipsis(&name, max_chars))
        .unwrap_or_else(|| text::abbreviate_middle(peer_id, 14, 6))
}

pub fn message_preview(body: &str, max_chars: usize) -> String {
    if body.chars().count() <= max_chars {
        body.to_string()
    } else {
        format!("{}...", body.chars().take(max_chars).collect::<String>())
    }
}

pub fn update_recent_peer(
    recent_peers: &mut Vec<PeerInfo>,
    discovered_peers: &[PeerInfo],
    target_id: Option<&str>,
) {
    if let Some(target_id) = target_id
        && let Some(peer) = discovered_peers.iter().find(|peer| peer.id == target_id).cloned()
    {
        recent_peers.retain(|recent| recent.id != peer.id);
        recent_peers.insert(0, peer);
    }
}

pub fn current_route_messages_peer(route: &Route) -> Option<Option<String>> {
    match route {
        Route::Messages { peer_id } => Some(peer_id.clone()),
        _ => None,
    }
}

pub fn back_queue_push<T>(queue: &mut VecDeque<T>, item: T) {
    queue.push_front(item);
}

pub fn reduce_contacts(state: &mut AppState, action: ContactsAction) -> Vec<AppCommand> {
    match action {
        ContactsAction::LoadRequested => vec![AppCommand::LoadContacts],
        ContactsAction::SaveRequested { peer_id, name, address } => {
            vec![AppCommand::SaveContact { peer_id, name, address }]
        }
        ContactsAction::DeleteRequested(id) => vec![AppCommand::DeleteContact { id }],
        ContactsAction::UpdateAddressRequested { id, new_address } => {
            let Some(contact) = state.contacts.contacts.iter().find(|contact| contact.id == id)
            else {
                return vec![notify_error("Contact not found.")];
            };

            vec![
                notify_info(format!("Updating address for {}...", contact.name)),
                AppCommand::UpdateContactAddress {
                    id: contact.id,
                    peer_id: contact.peer_id.clone(),
                    name: contact.name.clone(),
                    address: new_address,
                },
            ]
        }
        ContactsAction::Loaded(result) => match result {
            Ok(contacts) => {
                state.contacts.contacts = contacts;
                Vec::new()
            }
            Err(err) => vec![notify_error(format!("Unable to load contacts: {}", err))],
        },
        ContactsAction::Saved(result) => match result {
            Ok(contacts) => {
                state.contacts.contacts = contacts;
                vec![notify_success("Contact saved.")]
            }
            Err(err) => vec![notify_error(format!("Save Failed: {}", err))],
        },
        ContactsAction::Deleted(result) => match result {
            Ok(contacts) => {
                state.contacts.contacts = contacts;
                vec![notify_success("Contact deleted.")]
            }
            Err(err) => vec![notify_error(format!("Delete Failed: {}", err))],
        },
        ContactsAction::Updated(result) => match result {
            Ok(contacts) => {
                state.contacts.contacts = contacts;
                vec![notify_success("Contact updated.")]
            }
            Err(err) => vec![notify_error(format!("Update Failed: {}", err))],
        },
    }
}

pub fn reduce_messaging(state: &mut AppState, action: MessagingAction) -> Vec<AppCommand> {
    match action {
        MessagingAction::ServiceReady { summaries } => {
            state.services.messaging_ready = true;
            update_messaging_summaries(state, summaries);
            ensure_active_conversation(state)
        }
        MessagingAction::ServiceInitFailed(err) => {
            vec![notify_error(format!("Messaging service failed to initialize: {}", err))]
        }
        MessagingAction::SendRequested { peer_id, address, body } => {
            vec![AppCommand::SendMessage { peer_id, address, body }]
        }
        MessagingAction::SendCompleted(result) => match result {
            Ok(peer_id) => vec![AppCommand::ClearMessageDraft(peer_id)],
            Err(err) => vec![notify_error(err)],
        },
        MessagingAction::Event { event, summaries } => {
            reduce_messaging_event(state, event, summaries)
        }
        MessagingAction::ActiveConversationSelected(peer_id) => {
            state.messaging.active_peer_id = peer_id;
            vec![AppCommand::RefreshActiveConversation]
        }
        MessagingAction::ActiveConversationLoaded(messages) => {
            state.messaging.active_messages = messages;
            Vec::new()
        }
    }
}

pub fn reduce_call_action(state: &mut AppState, action: CallAction) -> Vec<AppCommand> {
    match action {
        CallAction::AcceptRequested => {
            state.session.incoming_call_id = None;
            state.session.incoming_call_timeout = None;
            state.session.call_connected = false;
            vec![AppCommand::AcceptCall]
        }
        CallAction::DeclineRequested => {
            clear_session(state);
            vec![AppCommand::DeclineCall]
        }
        CallAction::StartRequested(target) => {
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

            vec![AppCommand::StartCall { target }]
        }
    }
}

pub fn reduce_service(state: &mut AppState, action: ServiceAction) -> Vec<AppCommand> {
    match action {
        ServiceAction::CallServiceReady {
            local_peer_id,
            signaling_port,
            persist_local_peer_id,
        } => {
            state.services.call_ready = true;
            state.networking.local_peer_id = Some(local_peer_id.clone());

            let mut commands = Vec::new();
            if persist_local_peer_id {
                state.config.identity.peer_id = Some(local_peer_id.clone());
                commands.push(AppCommand::SaveConfig);
            }

            commands.push(AppCommand::InitializeDiscovery { local_peer_id, signaling_port });
            if should_initialize_messaging(state) {
                commands.push(AppCommand::InitializeMessaging);
            }
            commands
        }
        ServiceAction::CallServiceInitFailed(err) => {
            vec![notify_error(format!("Call service failed to initialize: {}", err))]
        }
        ServiceAction::DiscoveryServiceReady => {
            state.services.discovery_ready = true;
            Vec::new()
        }
        ServiceAction::DiscoveryServiceInitFailed(err) => {
            vec![notify_error(format!("Discovery service failed to initialize: {}", err))]
        }
        ServiceAction::DatabaseReady => {
            state.services.database_ready = true;

            let mut commands = vec![AppCommand::LoadContacts];
            if should_initialize_messaging(state) {
                commands.push(AppCommand::InitializeMessaging);
            }
            commands
        }
        ServiceAction::DatabaseInitFailed(err) => {
            vec![notify_error(format!("DB Failed: {}", err))]
        }
        ServiceAction::CallEvent(event) => reduce_call_event(state, event),
        ServiceAction::DiscoveryEvent(event) => {
            apply_discovery_event(state, event);
            Vec::new()
        }
        ServiceAction::PeerFound(peer) => {
            maybe_upsert_peer(state, peer);
            Vec::new()
        }
    }
}

pub fn should_initialize_messaging(state: &AppState) -> bool {
    state.services.database_ready && state.services.call_ready && !state.services.messaging_ready
}

pub fn ensure_active_conversation(state: &mut AppState) -> Vec<AppCommand> {
    if state.messaging.active_peer_id.is_none() {
        state.messaging.active_peer_id = resolve_selected_peer_id(
            &state.messaging.summaries,
            state.messaging.active_peer_id.clone(),
        );
    }

    vec![AppCommand::RefreshActiveConversation]
}

fn reduce_messaging_event(
    state: &mut AppState,
    event: MessagingEvent,
    summaries: Arc<Vec<ConversationSummary>>,
) -> Vec<AppCommand> {
    update_messaging_summaries(state, summaries);

    match event {
        MessagingEvent::ConversationUpdated { peer_id } => {
            let mut commands = maybe_select_first_conversation(state, &peer_id);
            if state.messaging.active_peer_id.as_deref() == Some(peer_id.as_str()) {
                commands.push(AppCommand::RefreshActiveConversation);
            }
            commands
        }
        MessagingEvent::IncomingMessage { peer_id, body } => {
            let mut commands = maybe_select_first_conversation(state, &peer_id);
            if state.messaging.active_peer_id.as_deref() == Some(peer_id.as_str()) {
                commands.push(AppCommand::RefreshActiveConversation);
            } else {
                commands.push(notify_info(format!(
                    "New message from {}: {}",
                    peer_label(
                        &state.contacts.contacts,
                        &state.networking.discovered_peers,
                        &peer_id,
                    ),
                    message_preview(&body, 32)
                )));
            }
            commands
        }
    }
}

fn maybe_select_first_conversation(state: &mut AppState, peer_id: &str) -> Vec<AppCommand> {
    if state.messaging.active_peer_id.is_none() {
        state.messaging.active_peer_id = Some(peer_id.to_string());
        vec![AppCommand::RefreshActiveConversation]
    } else {
        Vec::new()
    }
}

fn update_messaging_summaries(state: &mut AppState, summaries: Arc<Vec<ConversationSummary>>) {
    state.messaging.summaries = summaries;
    state.messaging.revision = state.messaging.revision.wrapping_add(1);
}

fn reduce_call_event(state: &mut AppState, event: CallEvent) -> Vec<AppCommand> {
    match event {
        CallEvent::IncomingCall { peer_id } => {
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
            Vec::new()
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
            vec![AppCommand::Navigate(Route::Call)]
        }
        CallEvent::CallEnded => {
            let had_target = state.session.target_id.is_some();
            clear_session(state);

            if had_target { vec![notify_info("Call ended.")] } else { Vec::new() }
        }
        CallEvent::RemoteStreamStarted | CallEvent::RemoteStreamEnded => Vec::new(),
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

fn clear_session(state: &mut AppState) {
    state.session.target_id = None;
    state.session.target_label = None;
    state.session.incoming_call_id = None;
    state.session.incoming_call_timeout = None;
    state.session.call_connected = false;
}

fn notify_error(message: impl Into<String>) -> AppCommand {
    AppCommand::Notify { level: NotificationLevel::Error, message: message.into() }
}

fn notify_info(message: impl Into<String>) -> AppCommand {
    AppCommand::Notify { level: NotificationLevel::Info, message: message.into() }
}

fn notify_success(message: impl Into<String>) -> AppCommand {
    AppCommand::Notify { level: NotificationLevel::Success, message: message.into() }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{
        config::Config,
        messaging::{MessageDirection, MessageStatus},
    };

    fn state() -> AppState {
        AppState {
            networking: NetworkingState {
                local_peer_id: None,
                discovered_peers: Vec::new(),
                recent_peers: Vec::new(),
            },
            session: SessionState {
                target_id: None,
                target_label: None,
                incoming_call_id: None,
                incoming_call_timeout: None,
                call_connected: false,
            },
            messaging: MessagingState {
                summaries: Arc::new(Vec::new()),
                active_peer_id: None,
                active_messages: Arc::new(Vec::new()),
                revision: 0,
            },
            contacts: ContactsState { contacts: Arc::new(Vec::new()) },
            config: Config::default(),
            services: ServicesState::default(),
        }
    }

    #[test]
    fn database_ready_requests_contacts() {
        let mut state = state();

        let commands = reduce_service(&mut state, ServiceAction::DatabaseReady);

        assert!(state.services.database_ready);
        assert!(commands.iter().any(|command| matches!(command, AppCommand::LoadContacts)));
    }

    #[test]
    fn incoming_message_selects_first_conversation_and_refreshes() {
        let mut state = state();
        let summaries = Arc::new(vec![ConversationSummary {
            peer_id: "peer-a".into(),
            last_message_id: 1,
            last_message_body: "hello".into(),
            last_message_direction: MessageDirection::Incoming,
            last_message_status: MessageStatus::Delivered,
            last_message_at: Utc::now(),
        }]);

        let commands = reduce_messaging(
            &mut state,
            MessagingAction::Event {
                event: MessagingEvent::IncomingMessage {
                    peer_id: "peer-a".into(),
                    body: "hello".into(),
                },
                summaries,
            },
        );

        assert_eq!(state.messaging.active_peer_id.as_deref(), Some("peer-a"));
        assert!(
            commands.iter().any(|command| matches!(command, AppCommand::RefreshActiveConversation))
        );
    }

    #[test]
    fn contact_update_uses_current_contact_snapshot() {
        let mut state = state();
        state.contacts.contacts = Arc::new(vec![Contact {
            id: 7,
            peer_id: "peer-a".into(),
            name: "Peer A".into(),
            address: None,
        }]);

        let commands = reduce_contacts(
            &mut state,
            ContactsAction::UpdateAddressRequested { id: 7, new_address: "127.0.0.1:9000".into() },
        );

        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::UpdateContactAddress { id, address, .. }
            if *id == 7 && address == "127.0.0.1:9000"
        )));
    }

    #[test]
    fn active_conversation_load_replaces_messages() {
        let mut state = state();
        let messages = Arc::new(vec![ConversationMessage {
            id: 1,
            message_id: "msg-1".into(),
            peer_id: "peer-a".into(),
            direction: MessageDirection::Incoming,
            body: "hello".into(),
            status: MessageStatus::Delivered,
            created_at: Utc::now(),
            delivered_at: None,
        }]);

        let commands = reduce_messaging(
            &mut state,
            MessagingAction::ActiveConversationLoaded(messages.clone()),
        );

        assert!(commands.is_empty());
        assert_eq!(state.messaging.active_messages.len(), 1);
        assert_eq!(state.messaging.active_messages[0].body, "hello");
    }

    #[test]
    fn call_service_ready_tracks_runtime_local_peer_id_without_persisting_when_disabled() {
        let mut state = state();
        state.config.identity.peer_id = Some("persisted-peer".into());

        let commands = reduce_service(
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
        assert!(!commands.iter().any(|command| matches!(command, AppCommand::SaveConfig)));
    }

    #[test]
    fn peer_found_ignores_runtime_local_peer_id_even_when_config_is_stale() {
        let mut state = state();
        state.config.identity.peer_id = Some("persisted-peer".into());
        state.networking.local_peer_id = Some("runtime-peer".into());

        let commands = reduce_service(
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

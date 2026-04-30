use std::net::SocketAddr;

use super::Route;
use crate::communication::call::CallTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Error,
    Info,
    Success,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    Notify {
        level: NotificationLevel,
        message: String,
    },
    SaveConfig {
        success_message: Option<String>,
        error_message: String,
    },
    ApplyCaptureReadback {
        enabled: bool,
    },
    RetryStartup,
    Navigate(Route),
    LoadContacts,
    SaveContact {
        peer_id: String,
        name: String,
        address: Option<String>,
        trusted_public_key: Option<String>,
    },
    DeleteContact {
        id: i64,
    },
    UpdateContact {
        id: i64,
        peer_id: String,
        name: String,
        address: Option<String>,
        trusted_public_key: Option<String>,
    },
    AcceptCall,
    DeclineCall,
    StartCall {
        target: CallTarget,
    },
    SendMessage {
        peer_id: String,
        address: SocketAddr,
        body: String,
    },
    InitializeDiscovery {
        local_peer_id: String,
        signaling_port: u16,
    },
    InitializeMessaging,
    RefreshActiveConversation,
    ClearMessageDraft(String),
}

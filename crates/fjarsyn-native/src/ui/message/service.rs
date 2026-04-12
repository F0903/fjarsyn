use std::{net::SocketAddr, sync::Arc};

use fjarsyn_core::{
    networking::{
        discovery::{DiscoveryEvent, PeerInfo},
        webrtc::WebRTCError,
    },
    services::{
        call_service::{CallEvent, CallService},
        contacts_service::Contact,
        discovery_service::{DiscoveryService, DiscoveryServiceError},
        messaging_service::{
            ConversationMessage, MessagingError, MessagingEvent, MessagingService,
        },
    },
};

#[derive(Debug, Clone)]
pub enum CallServiceMessage {
    CallServiceInitialized(Result<Arc<CallService>, Arc<WebRTCError>>),
    DiscoveryServiceInitialized(Result<Arc<DiscoveryService>, Arc<DiscoveryServiceError>>),
    CallEvent(CallEvent),
    DiscoveryEvent(DiscoveryEvent),
    PeerFound(PeerInfo),
}

#[derive(Debug, Clone)]
pub enum ContactsServiceMessage {
    LoadContacts,
    ContactsLoaded(Result<Arc<Vec<Contact>>, Arc<fjarsyn_core::Error>>),
    SaveContact { peer_id: String, name: String, address: Option<String> },
    DeleteContact(i64),
    ContactSaved(Result<Arc<Vec<Contact>>, Arc<fjarsyn_core::Error>>),
    ContactDeleted(Result<Arc<Vec<Contact>>, Arc<fjarsyn_core::Error>>),
    ContactUpdated(Result<Arc<Vec<Contact>>, Arc<fjarsyn_core::Error>>),
    UpdateContactAddress { id: i64, new_address: String },
}

#[derive(Debug, Clone)]
pub enum MessagingServiceMessage {
    ServiceInitialized(Result<Arc<MessagingService>, Arc<MessagingError>>),
    Event(MessagingEvent),
    SendMessage { peer_id: String, address: SocketAddr, body: String },
    MessageSent(Result<String, Arc<MessagingError>>),
    ActiveConversationLoaded(Arc<Vec<ConversationMessage>>),
}

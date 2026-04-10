use std::{net::SocketAddr, sync::Arc};

use crate::{
    networking::discovery::{DiscoveryEvent, PeerInfo},
    services::{
        call_service::CallEvent,
        messaging_service::{MessagingError, MessagingEvent, MessagingService},
    },
};

#[derive(Debug, Clone)]
pub enum CallServiceMessage {
    CallServiceInitialized(
        Result<
            Arc<crate::services::call_service::CallService>,
            Arc<crate::networking::webrtc::WebRTCError>,
        >,
    ),
    CallEvent(CallEvent),
    DiscoveryEvent(DiscoveryEvent),
    PeerFound(PeerInfo),
    PeerRemoved(String),
}

#[derive(Debug, Clone)]
pub enum ContactsServiceMessage {
    LoadContacts,
    SaveContact { peer_id: String, name: String, address: Option<String> },
    DeleteContact(i64),
    ContactSaved(Result<i64, Arc<crate::Error>>),
    ContactDeleted(Result<(), Arc<crate::Error>>),
    UpdateContactAddress { id: i64, new_address: String },
    UpdateContactAddressConfirmed(i64, String),
}

#[derive(Debug, Clone)]
pub enum MessagingServiceMessage {
    Initialize,
    ServiceInitialized(Result<Arc<MessagingService>, Arc<MessagingError>>),
    Event(MessagingEvent),
    SendMessage { peer_id: String, address: SocketAddr, body: String },
}

use std::sync::Arc;

use bytes::Bytes;

use crate::{
    networking::discovery::{DiscoveryEvent, PeerInfo},
    services::call_service::CallEvent,
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
    PacketReceived(Bytes),
}

#[derive(Debug, Clone)]
pub enum ContactsServiceMessage {
    LoadContacts,
    ContactsLoaded(Result<Vec<crate::database::Contact>, Arc<crate::Error>>),
    SaveContact { peer_id: String, name: String, address: Option<String> },
    DeleteContact(i64),
    ContactSaved(Result<i64, Arc<crate::Error>>),
    ContactDeleted(Result<(), Arc<crate::Error>>),
    UpdateContactAddress { id: i64, new_address: String },
    UpdateContactAddressConfirmed(i64, String),
}

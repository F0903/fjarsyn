use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use fjarsyn_core::{
    pairing::VerifiedPeerIdentity,
    services::contact_trust_service::{
        ContactRefreshOutcome, ContactTrustError, ContactTrustOutcome,
    },
};

use crate::ui::runtime::{RuntimeEvent, RuntimeSlot};

/// Process-unique correlation token for a contact service request and result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactOperationId(u64);

impl ContactOperationId {
    pub(crate) fn next() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeMessage {
    Initialized(Result<RuntimeSlot, Arc<String>>),
    Event(RuntimeEvent),
    ShutdownFinished(Result<(), Arc<String>>),
    RestartFinished {
        shutdown_warning: Option<Arc<String>>,
        launch_result: Result<(), Arc<String>>,
    },
}

#[derive(Debug, Clone)]
pub enum ContactsServiceMessage {
    LoadContacts,
    ContactsLoaded(Result<ContactRefreshOutcome, Arc<ContactTrustError>>),
    SaveContact {
        operation_id: ContactOperationId,
        name: String,
        identity: VerifiedPeerIdentity,
    },
    DeleteContact {
        operation_id: ContactOperationId,
        id: i64,
    },
    ContactSaved {
        operation_id: ContactOperationId,
        result: Result<ContactTrustOutcome, Arc<ContactTrustError>>,
    },
    ContactDeleted {
        operation_id: ContactOperationId,
        id: i64,
        result: Result<ContactTrustOutcome, Arc<ContactTrustError>>,
    },
    ContactUpdated {
        operation_id: ContactOperationId,
        result: Result<ContactTrustOutcome, Arc<ContactTrustError>>,
    },
    UpdateContactVerifiedIdentity {
        operation_id: ContactOperationId,
        id: i64,
        identity: VerifiedPeerIdentity,
    },
}

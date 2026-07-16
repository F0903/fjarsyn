use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::contacts_service::{Contact, ContactProjection, ContactsService};
use crate::{
    Error,
    pairing::VerifiedPeerIdentity,
    peer_session::{PeerId, PeerSessionError, PeerSessionServiceHandle, TrustBarrierOwnerId},
};

/// Application boundary for contact writes that affect peer authentication.
///
/// The contact repository/cache remains the trusted-peer resolver, while this
/// service serializes application mutations with the peer-session admission
/// barrier. Screens must never coordinate those two owners themselves.
#[derive(Clone)]
pub struct ContactTrustService {
    contacts: Arc<ContactsService>,
    sessions: Arc<dyn PeerAdmissionControl>,
    barrier_owner: TrustBarrierOwnerId,
    local_peer_id: PeerId,
    operations: Arc<Mutex<ContactTrustState>>,
}

#[async_trait]
trait PeerAdmissionControl: Send + Sync {
    async fn ensure_trust_suspended(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), PeerSessionError>;
    async fn release_trust_suspension(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), PeerSessionError>;
}

#[async_trait]
impl PeerAdmissionControl for PeerSessionServiceHandle {
    async fn ensure_trust_suspended(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), PeerSessionError> {
        PeerSessionServiceHandle::ensure_trust_suspended(self, peer_id, owner_id).await
    }

    async fn release_trust_suspension(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), PeerSessionError> {
        PeerSessionServiceHandle::release_trust_suspension(self, peer_id, owner_id).await
    }
}

#[derive(Debug, Default)]
struct ContactTrustState {
    pending: HashMap<PeerId, PendingTrustMutation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingTrustMutation {
    Update { original: Contact, intended_public_key: String },
    Delete { original: Contact },
}

impl PendingTrustMutation {
    fn original(&self) -> &Contact {
        match self {
            Self::Update { original, .. } | Self::Delete { original } => original,
        }
    }

    fn peer_id(&self) -> &PeerId {
        &self.original().peer_id
    }

    fn operation_name(&self) -> &'static str {
        match self {
            Self::Update { .. } => "identity update",
            Self::Delete { .. } => "contact deletion",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingResolution {
    Applied,
    NotApplied,
    Unknown,
}

/// Authoritative contact projection after a committed trust mutation.
///
/// A session-admission warning does not roll back the committed identity. The
/// caller must apply `contacts` and surface the warning; admission remains
/// fail-closed until the peer-session owner can be recovered or restarted.
#[derive(Debug, Clone)]
pub struct ContactTrustOutcome {
    pub projection: ContactProjection,
    pub admission_warning: Option<PeerSessionError>,
}

/// Authoritative contact projection produced by a refresh, together with every
/// peer whose now-definitive trust-mutation barrier could not be released.
/// Those barriers remain retained and a later refresh can retry recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactRefreshOutcome {
    pub projection: ContactProjection,
    pub admission_warnings: Vec<PeerAdmissionWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAdmissionWarning {
    pub peer_id: PeerId,
    pub error: PeerSessionError,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ContactTrustError {
    #[error("the local peer identity {peer_id} cannot be imported as a contact")]
    SelfIdentity { peer_id: PeerId },
    #[error(
        "verified identity belongs to peer {actual}, but contact {contact_id} belongs to peer {expected}"
    )]
    PeerIdentityMismatch { contact_id: i64, expected: PeerId, actual: PeerId },
    #[error(transparent)]
    Contact(Arc<Error>),
    #[error(transparent)]
    Session(#[from] PeerSessionError),
    #[error(
        "contact operation failed ({operation}); restoring peer-session admission also failed ({recovery})"
    )]
    Recovery { operation: Arc<Error>, recovery: PeerSessionError },
    #[error(
        "contact operation outcome could not be proven ({operation}); peer sessions remain suspended: {reconciliation}"
    )]
    OutcomeUnknown { operation: Arc<Error>, reconciliation: String },
    #[error(
        "peer {peer_id} already has a pending {pending}; reconcile it before starting another trust mutation"
    )]
    PendingReconciliation { peer_id: PeerId, pending: &'static str },
}

impl From<Arc<Error>> for ContactTrustError {
    fn from(error: Arc<Error>) -> Self {
        Self::Contact(error)
    }
}

impl ContactTrustService {
    pub fn new(
        contacts: Arc<ContactsService>,
        sessions: PeerSessionServiceHandle,
        local_peer_id: PeerId,
    ) -> Self {
        Self {
            contacts,
            sessions: Arc::new(sessions),
            barrier_owner: TrustBarrierOwnerId::allocate(),
            local_peer_id,
            operations: Arc::new(Mutex::new(ContactTrustState::default())),
        }
    }

    #[cfg(test)]
    fn new_with_admission_control(
        contacts: Arc<ContactsService>,
        sessions: Arc<dyn PeerAdmissionControl>,
        local_peer_id: PeerId,
    ) -> Self {
        Self {
            contacts,
            sessions,
            barrier_owner: TrustBarrierOwnerId::allocate(),
            local_peer_id,
            operations: Arc::new(Mutex::new(ContactTrustState::default())),
        }
    }

    pub fn projection(&self) -> ContactProjection {
        self.contacts.projection()
    }

    pub fn contacts(&self) -> Arc<Vec<Contact>> {
        self.contacts.contacts()
    }

    pub async fn refresh(&self) -> Result<ContactRefreshOutcome, ContactTrustError> {
        let mut state = self.operations.lock().await;
        let projection = self.contacts.refresh().await?;
        let admission_warnings = self.release_definitive_pending(&mut state, &projection).await;
        Ok(ContactRefreshOutcome { projection, admission_warnings })
    }

    pub async fn create(
        &self,
        name: String,
        identity: VerifiedPeerIdentity,
    ) -> Result<ContactTrustOutcome, ContactTrustError> {
        let mut state = self.operations.lock().await;
        self.reject_self_identity(&identity)?;
        self.reconcile_before_create(&mut state, identity.peer_id()).await?;
        let projection = self
            .contacts
            .create(identity.peer_id().to_string(), name, identity.public_key_base64().to_owned())
            .await?;
        Ok(Self::outcome(projection, None))
    }

    pub async fn delete(&self, id: i64) -> Result<ContactTrustOutcome, ContactTrustError> {
        let mut state = self.operations.lock().await;
        let pending =
            match state.pending.values().find(|pending| pending.original().id == id).cloned() {
                Some(pending @ PendingTrustMutation::Delete { .. }) => pending,
                Some(pending) => return Err(Self::pending_conflict(&pending)),
                None => PendingTrustMutation::Delete { original: self.contact_by_id(id)? },
            };
        self.ensure_pending(&mut state, pending.clone()).await?;
        match self.contacts.delete(id).await {
            Ok(projection) => {
                Ok(self.finish_applied_mutation(&mut state, &pending, projection).await)
            }
            Err(operation) => self.reconcile_failed(&mut state, pending, operation).await,
        }
    }

    pub async fn update_verified_identity(
        &self,
        id: i64,
        identity: VerifiedPeerIdentity,
    ) -> Result<ContactTrustOutcome, ContactTrustError> {
        let mut state = self.operations.lock().await;
        self.reject_self_identity(&identity)?;
        let intended_public_key = identity.public_key_base64().to_owned();
        let pending = match state.pending.get(identity.peer_id()).cloned() {
            Some(pending) => match &pending {
                PendingTrustMutation::Update { original, intended_public_key: retained_key }
                    if original.id == id && retained_key == &intended_public_key =>
                {
                    pending
                }
                _ => return Err(Self::pending_conflict(&pending)),
            },
            None => {
                let contact = self.contact_by_id(id)?;
                if identity.peer_id() != &contact.peer_id {
                    return Err(ContactTrustError::PeerIdentityMismatch {
                        contact_id: id,
                        expected: contact.peer_id,
                        actual: identity.peer_id().clone(),
                    });
                }
                PendingTrustMutation::Update {
                    original: contact,
                    intended_public_key: intended_public_key.clone(),
                }
            }
        };
        self.ensure_pending(&mut state, pending.clone()).await?;
        let original = pending.original();
        let operation = self
            .contacts
            .update(
                id,
                original.peer_id.to_string(),
                original.name.clone(),
                intended_public_key.clone(),
            )
            .await;
        match operation {
            Ok(projection) => {
                Ok(self.finish_applied_mutation(&mut state, &pending, projection).await)
            }
            Err(operation) => self.reconcile_failed(&mut state, pending, operation).await,
        }
    }

    async fn ensure_pending(
        &self,
        state: &mut ContactTrustState,
        pending: PendingTrustMutation,
    ) -> Result<(), ContactTrustError> {
        let peer_id = pending.peer_id().clone();
        match state.pending.get(&peer_id) {
            Some(existing) if existing == &pending => {}
            Some(existing) => return Err(Self::pending_conflict(existing)),
            None => {
                // Record intent before the async acquire. If the caller is
                // cancelled after the actor accepts the command, a retry can
                // reassert the idempotent barrier instead of losing ownership.
                state.pending.insert(peer_id.clone(), pending);
            }
        }
        self.sessions.ensure_trust_suspended(peer_id, self.barrier_owner).await?;
        Ok(())
    }

    async fn reconcile_before_create(
        &self,
        state: &mut ContactTrustState,
        peer_id: &PeerId,
    ) -> Result<(), ContactTrustError> {
        let Some(pending) = state.pending.get(peer_id).cloned() else {
            return Ok(());
        };
        let projection = self.contacts.refresh().await?;
        if Self::pending_resolution(&pending, &projection) != PendingResolution::Unknown
            && self
                .sessions
                .release_trust_suspension(peer_id.clone(), self.barrier_owner)
                .await
                .is_ok()
        {
            state.pending.remove(peer_id);
            return Ok(());
        }
        Err(Self::pending_conflict(&pending))
    }

    async fn reconcile_failed(
        &self,
        state: &mut ContactTrustState,
        pending: PendingTrustMutation,
        operation: Arc<Error>,
    ) -> Result<ContactTrustOutcome, ContactTrustError> {
        let projection = self.contacts.refresh().await.map_err(|reconciliation| {
            ContactTrustError::OutcomeUnknown {
                operation: operation.clone(),
                reconciliation: reconciliation.to_string(),
            }
        })?;

        match Self::pending_resolution(&pending, &projection) {
            PendingResolution::Applied => {
                Ok(self.finish_applied_mutation(state, &pending, projection).await)
            }
            PendingResolution::NotApplied => {
                self.finish_non_applied_mutation(state, &pending, operation).await
            }
            PendingResolution::Unknown => Err(ContactTrustError::OutcomeUnknown {
                operation,
                reconciliation: format!(
                    "{} for contact {} is neither the original nor intended state",
                    pending.operation_name(),
                    pending.original().id,
                ),
            }),
        }
    }

    fn pending_resolution(
        pending: &PendingTrustMutation,
        projection: &ContactProjection,
    ) -> PendingResolution {
        let original = pending.original();
        let current = projection.contacts.iter().find(|contact| contact.id == original.id);
        match (pending, current) {
            (PendingTrustMutation::Update { intended_public_key, .. }, Some(current))
                if current.peer_id == original.peer_id
                    && current.trusted_public_key == *intended_public_key =>
            {
                PendingResolution::Applied
            }
            (PendingTrustMutation::Update { .. }, Some(current))
                if current.peer_id == original.peer_id
                    && current.trusted_public_key == original.trusted_public_key =>
            {
                PendingResolution::NotApplied
            }
            (PendingTrustMutation::Delete { .. }, None) => PendingResolution::Applied,
            (PendingTrustMutation::Delete { .. }, Some(current))
                if current.peer_id == original.peer_id
                    && current.trusted_public_key == original.trusted_public_key =>
            {
                PendingResolution::NotApplied
            }
            _ => PendingResolution::Unknown,
        }
    }

    async fn finish_applied_mutation(
        &self,
        state: &mut ContactTrustState,
        pending: &PendingTrustMutation,
        projection: ContactProjection,
    ) -> ContactTrustOutcome {
        let peer_id = pending.peer_id().clone();
        let admission_warning =
            self.sessions.release_trust_suspension(peer_id.clone(), self.barrier_owner).await.err();
        if admission_warning.is_none() {
            state.pending.remove(&peer_id);
        }
        Self::outcome(projection, admission_warning)
    }

    async fn finish_non_applied_mutation(
        &self,
        state: &mut ContactTrustState,
        pending: &PendingTrustMutation,
        operation: Arc<Error>,
    ) -> Result<ContactTrustOutcome, ContactTrustError> {
        let peer_id = pending.peer_id().clone();
        match self.sessions.release_trust_suspension(peer_id.clone(), self.barrier_owner).await {
            Ok(()) => {
                state.pending.remove(&peer_id);
                Err(ContactTrustError::Contact(operation))
            }
            Err(recovery) => Err(ContactTrustError::Recovery { operation, recovery }),
        }
    }

    async fn release_definitive_pending(
        &self,
        state: &mut ContactTrustState,
        projection: &ContactProjection,
    ) -> Vec<PeerAdmissionWarning> {
        let mut admission_warnings = Vec::new();
        let pending = state.pending.values().cloned().collect::<Vec<_>>();
        for pending in pending {
            if Self::pending_resolution(&pending, projection) == PendingResolution::Unknown {
                continue;
            }
            let peer_id = pending.peer_id().clone();
            if let Err(error) =
                self.sessions.release_trust_suspension(peer_id.clone(), self.barrier_owner).await
            {
                tracing::warn!(%peer_id, %error, "contact trust reconciliation remains suspended");
                admission_warnings.push(PeerAdmissionWarning { peer_id, error });
            } else {
                state.pending.remove(&peer_id);
            }
        }
        admission_warnings
    }

    fn pending_conflict(pending: &PendingTrustMutation) -> ContactTrustError {
        ContactTrustError::PendingReconciliation {
            peer_id: pending.peer_id().clone(),
            pending: pending.operation_name(),
        }
    }

    fn outcome(
        projection: ContactProjection,
        admission_warning: Option<PeerSessionError>,
    ) -> ContactTrustOutcome {
        ContactTrustOutcome { projection, admission_warning }
    }

    fn reject_self_identity(
        &self,
        identity: &VerifiedPeerIdentity,
    ) -> Result<(), ContactTrustError> {
        if identity.peer_id() == &self.local_peer_id {
            return Err(ContactTrustError::SelfIdentity { peer_id: self.local_peer_id.clone() });
        }
        Ok(())
    }

    fn contact_by_id(&self, id: i64) -> Result<Contact, ContactTrustError> {
        self.contacts.contacts().iter().find(|contact| contact.id == id).cloned().ok_or_else(|| {
            ContactTrustError::Contact(Arc::new(Error::RecordNotFound { entity: "contact", id }))
        })
    }
}

#[cfg(test)]
mod tests;

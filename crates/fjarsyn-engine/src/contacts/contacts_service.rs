use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;

use super::{
    AdmissionControl, AdmissionWarning, Contact, Directory, DirectoryError, Error, Outcome,
    Projection, RefreshOutcome, StoreError,
};
use crate::{
    identity::PeerId,
    pairing::VerifiedPeerIdentity,
    peer_session::{self, TrustBarrierOwnerId},
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingMutation {
    Update { original: Contact, intended_public_key: String },
    Delete { original: Contact },
}

impl PendingMutation {
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

#[derive(Debug, Default)]
struct State {
    pending: HashMap<PeerId, PendingMutation>,
}

/// Application boundary for contact writes that affect peer authentication.
///
/// The contact store/cache remains the trusted-peer resolver, while this
/// service serializes application mutations with the peer-session admission
/// barrier. Screens must never coordinate those two owners themselves.
#[derive(Clone)]
pub struct ContactsService {
    directory: Arc<Directory>,
    admission: Arc<dyn AdmissionControl>,
    barrier_owner: TrustBarrierOwnerId,
    local_peer_id: PeerId,
    operations: Arc<Mutex<State>>,
}

impl ContactsService {
    pub(crate) fn new(
        directory: Arc<Directory>,
        sessions: peer_session::ServiceHandle,
        local_peer_id: PeerId,
    ) -> Self {
        Self {
            directory,
            admission: Arc::new(sessions),
            barrier_owner: TrustBarrierOwnerId::allocate(),
            local_peer_id,
            operations: Arc::new(Mutex::new(State::default())),
        }
    }

    #[cfg(test)]
    pub(super) fn new_with_admission_control(
        directory: Arc<Directory>,
        admission: Arc<dyn AdmissionControl>,
        local_peer_id: PeerId,
    ) -> Self {
        Self {
            directory,
            admission,
            barrier_owner: TrustBarrierOwnerId::allocate(),
            local_peer_id,
            operations: Arc::new(Mutex::new(State::default())),
        }
    }

    pub fn projection(&self) -> Projection {
        self.directory.projection()
    }

    pub fn contacts(&self) -> Arc<Vec<Contact>> {
        self.directory.contacts()
    }

    pub async fn refresh(&self) -> Result<RefreshOutcome, Error> {
        let mut state = self.operations.lock().await;
        let projection = self.directory.refresh().await?;
        let admission_warnings = self.release_definitive_pending(&mut state, &projection).await;
        Ok(RefreshOutcome { projection, admission_warnings })
    }

    pub async fn create(
        &self,
        name: String,
        identity: VerifiedPeerIdentity,
    ) -> Result<Outcome, Error> {
        let mut state = self.operations.lock().await;
        self.reject_self_identity(&identity)?;
        self.reconcile_before_create(&mut state, identity.peer_id()).await?;
        let projection = self
            .directory
            .create(identity.peer_id().to_string(), name, identity.public_key_base64().to_owned())
            .await?;
        Ok(Outcome { projection, admission_warning: None })
    }

    pub async fn delete(&self, id: i64) -> Result<Outcome, Error> {
        let mut state = self.operations.lock().await;
        let pending =
            match state.pending.values().find(|pending| pending.original().id == id).cloned() {
                Some(pending @ PendingMutation::Delete { .. }) => pending,
                Some(pending) => return Err(Self::pending_conflict(&pending)),
                None => PendingMutation::Delete { original: self.contact_by_id(id)? },
            };
        self.ensure_pending(&mut state, pending.clone()).await?;
        match self.directory.delete(id).await {
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
    ) -> Result<Outcome, Error> {
        let mut state = self.operations.lock().await;
        self.reject_self_identity(&identity)?;
        let intended_public_key = identity.public_key_base64().to_owned();
        let pending = match state.pending.get(identity.peer_id()).cloned() {
            Some(pending) => match &pending {
                PendingMutation::Update { original, intended_public_key: retained_key }
                    if original.id == id && retained_key == &intended_public_key =>
                {
                    pending
                }
                _ => return Err(Self::pending_conflict(&pending)),
            },
            None => {
                let contact = self.contact_by_id(id)?;
                if identity.peer_id() != &contact.peer_id {
                    return Err(Error::PeerIdentityMismatch {
                        contact_id: id,
                        expected: contact.peer_id,
                        actual: identity.peer_id().clone(),
                    });
                }
                PendingMutation::Update {
                    original: contact,
                    intended_public_key: intended_public_key.clone(),
                }
            }
        };
        self.ensure_pending(&mut state, pending.clone()).await?;
        let original = pending.original();
        let operation = self
            .directory
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
        state: &mut State,
        pending: PendingMutation,
    ) -> Result<(), Error> {
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
        self.admission.ensure_trust_suspended(peer_id, self.barrier_owner).await?;
        Ok(())
    }

    async fn reconcile_before_create(
        &self,
        state: &mut State,
        peer_id: &PeerId,
    ) -> Result<(), Error> {
        let Some(pending) = state.pending.get(peer_id).cloned() else {
            return Ok(());
        };
        let projection = self.directory.refresh().await?;
        if Self::pending_resolution(&pending, &projection) != PendingResolution::Unknown
            && self
                .admission
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
        state: &mut State,
        pending: PendingMutation,
        operation: Arc<DirectoryError>,
    ) -> Result<Outcome, Error> {
        let projection =
            self.directory.refresh().await.map_err(|reconciliation| Error::OutcomeUnknown {
                operation: operation.clone(),
                reconciliation: reconciliation.to_string(),
            })?;

        match Self::pending_resolution(&pending, &projection) {
            PendingResolution::Applied => {
                Ok(self.finish_applied_mutation(state, &pending, projection).await)
            }
            PendingResolution::NotApplied => {
                self.finish_non_applied_mutation(state, &pending, operation).await
            }
            PendingResolution::Unknown => Err(Error::OutcomeUnknown {
                operation,
                reconciliation: format!(
                    "{} for contact {} is neither the original nor intended state",
                    pending.operation_name(),
                    pending.original().id,
                ),
            }),
        }
    }

    fn pending_resolution(pending: &PendingMutation, projection: &Projection) -> PendingResolution {
        let original = pending.original();
        let current = projection.contacts.iter().find(|contact| contact.id == original.id);
        match (pending, current) {
            (PendingMutation::Update { intended_public_key, .. }, Some(current))
                if current.peer_id == original.peer_id
                    && current.trusted_public_key == *intended_public_key =>
            {
                PendingResolution::Applied
            }
            (PendingMutation::Update { .. }, Some(current))
                if current.peer_id == original.peer_id
                    && current.trusted_public_key == original.trusted_public_key =>
            {
                PendingResolution::NotApplied
            }
            (PendingMutation::Delete { .. }, None) => PendingResolution::Applied,
            (PendingMutation::Delete { .. }, Some(current))
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
        state: &mut State,
        pending: &PendingMutation,
        projection: Projection,
    ) -> Outcome {
        let peer_id = pending.peer_id().clone();
        let admission_warning = self
            .admission
            .release_trust_suspension(peer_id.clone(), self.barrier_owner)
            .await
            .err();
        if admission_warning.is_none() {
            state.pending.remove(&peer_id);
        }
        Outcome { projection, admission_warning }
    }

    async fn finish_non_applied_mutation(
        &self,
        state: &mut State,
        pending: &PendingMutation,
        operation: Arc<DirectoryError>,
    ) -> Result<Outcome, Error> {
        let peer_id = pending.peer_id().clone();
        match self.admission.release_trust_suspension(peer_id.clone(), self.barrier_owner).await {
            Ok(()) => {
                state.pending.remove(&peer_id);
                Err(Error::Contact(operation))
            }
            Err(recovery) => Err(Error::Recovery { operation, recovery }),
        }
    }

    async fn release_definitive_pending(
        &self,
        state: &mut State,
        projection: &Projection,
    ) -> Vec<AdmissionWarning> {
        let mut admission_warnings = Vec::new();
        let pending = state.pending.values().cloned().collect::<Vec<_>>();
        for pending in pending {
            if Self::pending_resolution(&pending, projection) == PendingResolution::Unknown {
                continue;
            }
            let peer_id = pending.peer_id().clone();
            if let Err(error) =
                self.admission.release_trust_suspension(peer_id.clone(), self.barrier_owner).await
            {
                tracing::warn!(%peer_id, %error, "contact trust reconciliation remains suspended");
                admission_warnings.push(AdmissionWarning { peer_id, error });
            } else {
                state.pending.remove(&peer_id);
            }
        }
        admission_warnings
    }

    fn pending_conflict(pending: &PendingMutation) -> Error {
        Error::PendingReconciliation {
            peer_id: pending.peer_id().clone(),
            pending: pending.operation_name(),
        }
    }

    fn reject_self_identity(&self, identity: &VerifiedPeerIdentity) -> Result<(), Error> {
        if identity.peer_id() == &self.local_peer_id {
            return Err(Error::SelfIdentity { peer_id: self.local_peer_id.clone() });
        }
        Ok(())
    }

    fn contact_by_id(&self, id: i64) -> Result<Contact, Error> {
        self.directory
            .contacts()
            .iter()
            .find(|contact| contact.id == id)
            .cloned()
            .ok_or_else(|| Error::Contact(Arc::new(StoreError::NotFound { id }.into())))
    }
}

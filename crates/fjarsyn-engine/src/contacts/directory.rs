use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{Contact, ContactRecord, Store, StoreError};
use crate::{
    identity::{PeerId, TrustedPeerIdentity},
    peer_session::{self, TrustedPeerResolver},
};

const MAX_CONTACT_NAME_BYTES: usize = 128;
static NEXT_PROJECTION_SOURCE_ID: AtomicU64 = AtomicU64::new(1);

/// Atomically captured contact projection. The source ID identifies the
/// originating directory generation, while revisions increase whenever that
/// generation's resolver cache is successfully replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    pub contacts: Arc<Vec<Contact>>,
    pub source_id: u64,
    pub revision: u64,
}

impl Projection {
    fn initial() -> Self {
        let source_id = NEXT_PROJECTION_SOURCE_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| next.checked_add(1))
            .expect("contact projection source ID space exhausted");
        Self { contacts: Arc::new(Vec::new()), source_id, revision: 0 }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DirectoryError {
    #[error("invalid contact: {0}")]
    InvalidContact(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Clone)]
pub(crate) struct Directory {
    store: Arc<dyn Store>,
    projection: Arc<RwLock<Projection>>,
    operations: Arc<Mutex<()>>,
}

impl Directory {
    /// Loads and validates the authoritative contact directory before making
    /// it available for trusted-peer resolution.
    pub(crate) async fn load(store: Arc<dyn Store>) -> Result<Self, Arc<DirectoryError>> {
        let directory = Self::new(store);
        directory.refresh().await?;
        Ok(directory)
    }

    pub(super) fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            projection: Arc::new(RwLock::new(Projection::initial())),
            operations: Arc::new(Mutex::new(())),
        }
    }

    pub(super) fn projection(&self) -> Projection {
        self.projection.read().unwrap().clone()
    }

    pub(super) fn contacts(&self) -> Arc<Vec<Contact>> {
        self.projection().contacts
    }

    pub(super) async fn refresh(&self) -> Result<Projection, Arc<DirectoryError>> {
        let _operation = self.operations.lock().await;
        self.refresh_locked().await
    }

    async fn refresh_locked(&self) -> Result<Projection, Arc<DirectoryError>> {
        let records = self.store.list().await.map_err(|error| Arc::new(error.into()))?;
        let contacts = records
            .into_iter()
            .map(Self::contact_from_record)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Arc::new)?;
        Ok(self.replace_contacts(contacts))
    }

    pub(super) async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<Projection, Arc<DirectoryError>> {
        let _operation = self.operations.lock().await;
        let (peer_id, name, trusted_public_key) =
            Self::validate_fields(peer_id, name, trusted_public_key).map_err(Arc::new)?;
        let record = self
            .store
            .create(peer_id.to_string(), name.clone(), trusted_public_key.clone())
            .await
            .map_err(|error| Arc::new(error.into()))?;
        let contact =
            Self::contact_from_committed_record(record, peer_id, name, trusted_public_key);
        let mut new_vec = self.contacts().as_ref().clone();
        new_vec.insert(0, contact);
        Ok(self.replace_contacts(new_vec))
    }

    pub(super) async fn delete(&self, id: i64) -> Result<Projection, Arc<DirectoryError>> {
        let _operation = self.operations.lock().await;
        self.store.delete(id).await.map_err(|error| Arc::new(error.into()))?;

        let mut new_vec = self.contacts().as_ref().clone();
        new_vec.retain(|contact| contact.id != id);
        Ok(self.replace_contacts(new_vec))
    }

    pub(super) async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<Projection, Arc<DirectoryError>> {
        let _operation = self.operations.lock().await;
        let (peer_id, name, trusted_public_key) =
            Self::validate_fields(peer_id, name, trusted_public_key).map_err(Arc::new)?;
        let record = self
            .store
            .update(id, peer_id.to_string(), name.clone(), trusted_public_key.clone())
            .await
            .map_err(|error| Arc::new(error.into()))?;
        let updated_contact =
            Self::contact_from_committed_record(record, peer_id, name, trusted_public_key);
        let mut new_vec = self.contacts().as_ref().clone();
        if let Some(contact) = new_vec.iter_mut().find(|contact| contact.id == id) {
            *contact = updated_contact;
        } else {
            new_vec.insert(0, updated_contact);
        }
        Ok(self.replace_contacts(new_vec))
    }

    fn contact_for_peer(&self, peer_id: &PeerId) -> Option<Contact> {
        self.projection
            .read()
            .unwrap()
            .contacts
            .iter()
            .find(|contact| &contact.peer_id == peer_id)
            .cloned()
    }

    fn replace_contacts(&self, contacts: Vec<Contact>) -> Projection {
        let mut projection = self.projection.write().unwrap();
        projection.revision =
            projection.revision.checked_add(1).expect("contact projection revision exhausted");
        projection.contacts = Arc::new(contacts);
        projection.clone()
    }

    fn validate_fields(
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<(PeerId, String, String), DirectoryError> {
        let peer_id = PeerId::new(peer_id)
            .map_err(|error| DirectoryError::InvalidContact(error.to_string()))?;
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(DirectoryError::InvalidContact("name cannot be empty".into()));
        }
        if name.len() > MAX_CONTACT_NAME_BYTES {
            return Err(DirectoryError::InvalidContact(format!(
                "name exceeds the {MAX_CONTACT_NAME_BYTES} byte limit"
            )));
        }
        let trusted_public_key = trusted_public_key.trim().to_owned();
        TrustedPeerIdentity::new(peer_id.clone(), &trusted_public_key)
            .validate()
            .map_err(|error| DirectoryError::InvalidContact(error.to_string()))?;

        Ok((peer_id, name, trusted_public_key))
    }

    fn contact_from_record(record: ContactRecord) -> Result<Contact, DirectoryError> {
        let (peer_id, name, trusted_public_key) =
            Self::validate_fields(record.peer_id, record.name, record.trusted_public_key)?;
        Ok(Contact {
            id: record.id,
            peer_id,
            name,
            trusted_public_key,
            created_at: record.created_at,
            updated_at: record.updated_at,
        })
    }

    /// The store returned this record from the same mutating persistence operation.
    /// Identity fields were validated before that operation, so applying them
    /// with the authoritative record metadata cannot introduce a fallible read or
    /// validation step after the commit.
    fn contact_from_committed_record(
        record: ContactRecord,
        peer_id: PeerId,
        name: String,
        trusted_public_key: String,
    ) -> Contact {
        Contact {
            id: record.id,
            peer_id,
            name,
            trusted_public_key,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[async_trait]
impl TrustedPeerResolver for Directory {
    async fn trusted_peer(
        &self,
        peer_id: &PeerId,
    ) -> Result<Option<TrustedPeerIdentity>, peer_session::Error> {
        Ok(self
            .contact_for_peer(peer_id)
            .map(|contact| TrustedPeerIdentity::new(contact.peer_id, contact.trusted_public_key)))
    }
}

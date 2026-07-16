use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    Error,
    identity::TrustedPeerIdentity,
    peer_session::{PeerId, PeerSessionError, TrustedPeerResolver},
    repositories::ContactsStore,
};

const MAX_CONTACT_NAME_BYTES: usize = 128;
static NEXT_PROJECTION_SOURCE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: i64,
    pub peer_id: PeerId,
    pub name: String,
    pub trusted_public_key: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Atomically captured contact projection. The source ID identifies the
/// `ContactsService` generation, while revisions increase whenever that
/// generation's resolver cache is successfully replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactProjection {
    pub contacts: Arc<Vec<Contact>>,
    pub source_id: u64,
    pub revision: u64,
}

impl ContactProjection {
    fn initial() -> Self {
        let source_id = NEXT_PROJECTION_SOURCE_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| next.checked_add(1))
            .expect("contact projection source ID space exhausted");
        Self { contacts: Arc::new(Vec::new()), source_id, revision: 0 }
    }
}

#[derive(Clone)]
pub struct ContactsService {
    repository: Arc<dyn ContactsStore>,
    projection: Arc<RwLock<ContactProjection>>,
    operations: Arc<Mutex<()>>,
}

impl ContactsService {
    pub fn new(repository: Arc<dyn ContactsStore>) -> Self {
        Self {
            repository,
            projection: Arc::new(RwLock::new(ContactProjection::initial())),
            operations: Arc::new(Mutex::new(())),
        }
    }

    pub fn projection(&self) -> ContactProjection {
        self.projection.read().unwrap().clone()
    }

    pub fn contacts(&self) -> Arc<Vec<Contact>> {
        self.projection().contacts
    }

    pub async fn refresh(&self) -> Result<ContactProjection, Arc<Error>> {
        let _operation = self.operations.lock().await;
        self.refresh_locked().await
    }

    async fn refresh_locked(&self) -> Result<ContactProjection, Arc<Error>> {
        let models = self.repository.list().await.map_err(Arc::new)?;
        let contacts = models
            .into_iter()
            .map(Self::contact_from_model)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Arc::new)?;
        Ok(self.replace_contacts(contacts))
    }

    pub(crate) async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactProjection, Arc<Error>> {
        let _operation = self.operations.lock().await;
        let (peer_id, name, trusted_public_key) =
            Self::validate_fields(peer_id, name, trusted_public_key).map_err(Arc::new)?;
        let model = self
            .repository
            .create(peer_id.to_string(), name.clone(), trusted_public_key.clone())
            .await
            .map_err(Arc::new)?;
        let contact = Self::contact_from_committed_model(model, peer_id, name, trusted_public_key);
        let mut new_vec = self.contacts().as_ref().clone();
        new_vec.insert(0, contact);
        Ok(self.replace_contacts(new_vec))
    }

    pub(crate) async fn delete(&self, id: i64) -> Result<ContactProjection, Arc<Error>> {
        let _operation = self.operations.lock().await;
        self.repository.delete(id).await.map_err(Arc::new)?;

        let mut new_vec = self.contacts().as_ref().clone();
        new_vec.retain(|contact| contact.id != id);
        Ok(self.replace_contacts(new_vec))
    }

    pub(crate) async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactProjection, Arc<Error>> {
        let _operation = self.operations.lock().await;
        let (peer_id, name, trusted_public_key) =
            Self::validate_fields(peer_id, name, trusted_public_key).map_err(Arc::new)?;
        let model = self
            .repository
            .update(id, peer_id.to_string(), name.clone(), trusted_public_key.clone())
            .await
            .map_err(Arc::new)?;
        let updated_contact =
            Self::contact_from_committed_model(model, peer_id, name, trusted_public_key);
        let mut new_vec = self.contacts().as_ref().clone();
        if let Some(contact) = new_vec.iter_mut().find(|contact| contact.id == id) {
            *contact = updated_contact;
        } else {
            new_vec.insert(0, updated_contact);
        }
        Ok(self.replace_contacts(new_vec))
    }

    pub fn contact_for_peer(&self, peer_id: &PeerId) -> Option<Contact> {
        self.projection
            .read()
            .unwrap()
            .contacts
            .iter()
            .find(|contact| &contact.peer_id == peer_id)
            .cloned()
    }

    fn replace_contacts(&self, contacts: Vec<Contact>) -> ContactProjection {
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
    ) -> Result<(PeerId, String, String), Error> {
        let peer_id =
            PeerId::new(peer_id).map_err(|error| Error::InvalidContact(error.to_string()))?;
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(Error::InvalidContact("name cannot be empty".into()));
        }
        if name.len() > MAX_CONTACT_NAME_BYTES {
            return Err(Error::InvalidContact(format!(
                "name exceeds the {MAX_CONTACT_NAME_BYTES} byte limit"
            )));
        }
        let trusted_public_key = trusted_public_key.trim().to_owned();
        TrustedPeerIdentity::new(peer_id.clone(), &trusted_public_key)
            .validate()
            .map_err(|error| Error::InvalidContact(error.to_string()))?;

        Ok((peer_id, name, trusted_public_key))
    }

    fn contact_from_model(model: crate::database::ContactModel) -> Result<Contact, Error> {
        let (peer_id, name, trusted_public_key) =
            Self::validate_fields(model.peer_id, model.name, model.trusted_public_key)?;
        Ok(Contact {
            id: model.id,
            peer_id,
            name,
            trusted_public_key,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }

    /// The repository returned this row from the same mutating SQL statement.
    /// Identity fields were validated before that statement, so applying them
    /// with the authoritative row metadata cannot introduce a fallible read or
    /// validation step after the commit.
    fn contact_from_committed_model(
        model: crate::database::ContactModel,
        peer_id: PeerId,
        name: String,
        trusted_public_key: String,
    ) -> Contact {
        Contact {
            id: model.id,
            peer_id,
            name,
            trusted_public_key,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

#[async_trait]
impl TrustedPeerResolver for ContactsService {
    async fn trusted_peer(
        &self,
        peer_id: &PeerId,
    ) -> Result<Option<TrustedPeerIdentity>, PeerSessionError> {
        Ok(self
            .contact_for_peer(peer_id)
            .map(|contact| TrustedPeerIdentity::new(contact.peer_id, contact.trusted_public_key)))
    }
}

#[cfg(test)]
mod tests;

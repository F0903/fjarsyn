use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, Ordering},
};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::Notify;

use super::ContactsService;
use crate::{database::ContactModel, identity::LocalPeerIdentity, repositories::ContactsStore};

#[derive(Default)]
struct FakeContactsStore {
    next_id: AtomicI64,
    contacts: Mutex<Vec<ContactModel>>,
}

struct BlockingListStore {
    inner: FakeContactsStore,
    block_next_list: AtomicBool,
    list_started: Notify,
    release_list: Notify,
}

impl BlockingListStore {
    fn with_contacts(contacts: Vec<ContactModel>) -> Self {
        Self {
            inner: FakeContactsStore::with_contacts(contacts),
            block_next_list: AtomicBool::new(false),
            list_started: Notify::new(),
            release_list: Notify::new(),
        }
    }
}

impl FakeContactsStore {
    fn with_contacts(contacts: Vec<ContactModel>) -> Self {
        let next_id = contacts.iter().map(|contact| contact.id).max().unwrap_or(0) + 1;
        Self { next_id: AtomicI64::new(next_id), contacts: Mutex::new(contacts) }
    }
}

#[async_trait]
impl ContactsStore for FakeContactsStore {
    async fn list(&self) -> Result<Vec<ContactModel>, crate::Error> {
        Ok(self.contacts.lock().unwrap().clone())
    }

    async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactModel, crate::Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let now = Utc::now();
        let model = ContactModel {
            id,
            peer_id,
            name,
            trusted_public_key,
            created_at: now,
            updated_at: now,
        };
        self.contacts.lock().unwrap().insert(0, model.clone());
        Ok(model)
    }

    async fn delete(&self, id: i64) -> Result<(), crate::Error> {
        self.contacts.lock().unwrap().retain(|contact| contact.id != id);
        Ok(())
    }

    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactModel, crate::Error> {
        if let Some(contact) =
            self.contacts.lock().unwrap().iter_mut().find(|contact| contact.id == id)
        {
            contact.peer_id = peer_id;
            contact.name = name;
            contact.trusted_public_key = trusted_public_key;
            contact.updated_at = Utc::now();
            return Ok(contact.clone());
        }
        Err(crate::Error::RecordNotFound { entity: "contact", id })
    }
}

#[async_trait]
impl ContactsStore for BlockingListStore {
    async fn list(&self) -> Result<Vec<ContactModel>, crate::Error> {
        let snapshot = self.inner.list().await?;
        if self.block_next_list.swap(false, Ordering::SeqCst) {
            self.list_started.notify_one();
            self.release_list.notified().await;
        }
        Ok(snapshot)
    }

    async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactModel, crate::Error> {
        self.inner.create(peer_id, name, trusted_public_key).await
    }

    async fn delete(&self, id: i64) -> Result<(), crate::Error> {
        self.inner.delete(id).await
    }

    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactModel, crate::Error> {
        self.inner.update(id, peer_id, name, trusted_public_key).await
    }
}

fn model(id: i64, peer_id: &str, name: &str) -> ContactModel {
    let now = Utc::now();
    ContactModel {
        id,
        peer_id: peer_id.to_string(),
        name: name.to_string(),
        trusted_public_key: valid_public_key(),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn refresh_uses_fake_store() {
    let service = ContactsService::new(Arc::new(FakeContactsStore::with_contacts(vec![
        model(2, "peer-b", "B"),
        model(1, "peer-a", "A"),
    ])));

    service.refresh().await.unwrap();

    let contacts = service.contacts();
    assert_eq!(contacts.len(), 2);
    assert_eq!(contacts[0].peer_id.as_str(), "peer-b");
    assert_eq!(contacts[0].name, "B");
    assert!(!contacts[0].trusted_public_key.is_empty());
    assert_eq!(contacts[1].peer_id.as_str(), "peer-a");
    assert_eq!(contacts[1].name, "A");
    assert!(!contacts[1].trusted_public_key.is_empty());
}

#[tokio::test]
async fn create_updates_cache_without_database_pool() {
    let service = ContactsService::new(Arc::new(FakeContactsStore::default()));

    let projection =
        service.create("peer-new".into(), "New Contact".into(), valid_public_key()).await.unwrap();

    let contacts = projection.contacts;
    assert_eq!(contacts[0].peer_id.as_str(), "peer-new");
    assert_eq!(contacts[0].name, "New Contact");
    assert!(!contacts[0].trusted_public_key.is_empty());
}

#[tokio::test]
async fn every_successful_cache_replacement_advances_the_atomic_projection_revision() {
    let service = ContactsService::new(Arc::new(FakeContactsStore::default()));
    let source_id = service.projection().source_id;
    assert_eq!(service.projection().revision, 0);

    let refreshed = service.refresh().await.unwrap();
    assert_eq!(refreshed.source_id, source_id);
    assert_eq!(refreshed.revision, 1);
    let created =
        service.create("peer-a".into(), "Alice".into(), valid_public_key()).await.unwrap();
    assert_eq!(created.revision, 2);
    let id = created.contacts[0].id;
    let updated =
        service.update(id, "peer-a".into(), "Alice".into(), valid_public_key()).await.unwrap();
    assert_eq!(updated.revision, 3);
    let deleted = service.delete(id).await.unwrap();
    assert_eq!(deleted.revision, 4);
    assert!(deleted.contacts.is_empty());
    assert_eq!(service.projection(), deleted);
}

#[tokio::test]
async fn a_replacement_service_uses_a_distinct_projection_source() {
    let retired = ContactsService::new(Arc::new(FakeContactsStore::default()));
    for _ in 0..3 {
        retired.refresh().await.unwrap();
    }
    let delayed_retired_projection = retired.projection();

    let replacement = ContactsService::new(Arc::new(FakeContactsStore::default()));
    let replacement_projection = replacement.projection();

    assert_ne!(delayed_retired_projection.source_id, replacement_projection.source_id);
    assert!(delayed_retired_projection.revision > replacement_projection.revision);
}

fn valid_public_key() -> String {
    LocalPeerIdentity::generate().public_key_base64()
}

#[tokio::test]
async fn update_refreshes_the_cached_contact_from_the_store() {
    let service = ContactsService::new(Arc::new(FakeContactsStore::with_contacts(vec![model(
        7, "peer-old", "Old Name",
    )])));
    service.refresh().await.unwrap();
    let created_at = service.contacts()[0].created_at;

    let new_key = valid_public_key();
    service.update(7, "peer-new".into(), "New Name".into(), new_key.clone()).await.unwrap();

    let contacts = service.contacts();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].peer_id.as_str(), "peer-new");
    assert_eq!(contacts[0].name, "New Name");
    assert_eq!(contacts[0].trusted_public_key, new_key);
    assert_eq!(contacts[0].created_at, created_at);
}

#[tokio::test]
async fn create_rejects_invalid_identity_fields_before_persistence() {
    let service = ContactsService::new(Arc::new(FakeContactsStore::default()));

    assert!(service.create(" ".into(), "Alice".into(), valid_public_key()).await.is_err());
    assert!(service.create("peer-a".into(), " ".into(), valid_public_key()).await.is_err());
    assert!(service.create("peer-a".into(), "Alice".into(), "not-a-key".into()).await.is_err());
    assert!(service.contacts().is_empty());
}

#[tokio::test]
async fn stale_refresh_cannot_resurrect_a_deleted_trusted_contact() {
    let store = Arc::new(BlockingListStore::with_contacts(vec![model(7, "peer-a", "Alice")]));
    let service = ContactsService::new(store.clone());
    service.refresh().await.unwrap();

    store.block_next_list.store(true, Ordering::SeqCst);
    let refresh_service = service.clone();
    let refresh = tokio::spawn(async move { refresh_service.refresh().await });
    store.list_started.notified().await;

    let delete_service = service.clone();
    let delete = tokio::spawn(async move { delete_service.delete(7).await });
    tokio::task::yield_now().await;
    store.release_list.notify_one();

    refresh.await.unwrap().unwrap();
    delete.await.unwrap().unwrap();
    assert!(service.contacts().is_empty());
}

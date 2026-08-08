use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, Ordering},
};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::Notify;

use super::{ContactRecord, Directory, Store, StoreError};
use crate::identity::LocalPeerIdentity;

#[derive(Default)]
struct FakeContactsStore {
    next_id: AtomicI64,
    contacts: Mutex<Vec<ContactRecord>>,
}

struct BlockingListStore {
    inner: FakeContactsStore,
    block_next_list: AtomicBool,
    list_started: Notify,
    release_list: Notify,
}

impl BlockingListStore {
    fn with_contacts(contacts: Vec<ContactRecord>) -> Self {
        Self {
            inner: FakeContactsStore::with_contacts(contacts),
            block_next_list: AtomicBool::new(false),
            list_started: Notify::new(),
            release_list: Notify::new(),
        }
    }
}

impl FakeContactsStore {
    fn with_contacts(contacts: Vec<ContactRecord>) -> Self {
        let next_id = contacts.iter().map(|contact| contact.id).max().unwrap_or(0) + 1;
        Self { next_id: AtomicI64::new(next_id), contacts: Mutex::new(contacts) }
    }
}

#[async_trait]
impl Store for FakeContactsStore {
    async fn list(&self) -> Result<Vec<ContactRecord>, StoreError> {
        Ok(self.contacts.lock().unwrap().clone())
    }

    async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let now = Utc::now();
        let model = ContactRecord {
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

    async fn delete(&self, id: i64) -> Result<(), StoreError> {
        self.contacts.lock().unwrap().retain(|contact| contact.id != id);
        Ok(())
    }

    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError> {
        if let Some(contact) =
            self.contacts.lock().unwrap().iter_mut().find(|contact| contact.id == id)
        {
            contact.peer_id = peer_id;
            contact.name = name;
            contact.trusted_public_key = trusted_public_key;
            contact.updated_at = Utc::now();
            return Ok(contact.clone());
        }
        Err(StoreError::NotFound { id })
    }
}

#[async_trait]
impl Store for BlockingListStore {
    async fn list(&self) -> Result<Vec<ContactRecord>, StoreError> {
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
    ) -> Result<ContactRecord, StoreError> {
        self.inner.create(peer_id, name, trusted_public_key).await
    }

    async fn delete(&self, id: i64) -> Result<(), StoreError> {
        self.inner.delete(id).await
    }

    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError> {
        self.inner.update(id, peer_id, name, trusted_public_key).await
    }
}

fn model(id: i64, peer_id: &str, name: &str) -> ContactRecord {
    let now = Utc::now();
    ContactRecord {
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
    let directory = Directory::new(Arc::new(FakeContactsStore::with_contacts(vec![
        model(2, "peer-b", "B"),
        model(1, "peer-a", "A"),
    ])));

    directory.refresh().await.unwrap();

    let contacts = directory.contacts();
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
    let directory = Directory::new(Arc::new(FakeContactsStore::default()));

    let projection = directory
        .create("peer-new".into(), "New Contact".into(), valid_public_key())
        .await
        .unwrap();

    let contacts = projection.contacts;
    assert_eq!(contacts[0].peer_id.as_str(), "peer-new");
    assert_eq!(contacts[0].name, "New Contact");
    assert!(!contacts[0].trusted_public_key.is_empty());
}

#[tokio::test]
async fn every_successful_cache_replacement_advances_the_atomic_projection_revision() {
    let directory = Directory::new(Arc::new(FakeContactsStore::default()));
    let source_id = directory.projection().source_id;
    assert_eq!(directory.projection().revision, 0);

    let refreshed = directory.refresh().await.unwrap();
    assert_eq!(refreshed.source_id, source_id);
    assert_eq!(refreshed.revision, 1);
    let created =
        directory.create("peer-a".into(), "Alice".into(), valid_public_key()).await.unwrap();
    assert_eq!(created.revision, 2);
    let id = created.contacts[0].id;
    let updated =
        directory.update(id, "peer-a".into(), "Alice".into(), valid_public_key()).await.unwrap();
    assert_eq!(updated.revision, 3);
    let deleted = directory.delete(id).await.unwrap();
    assert_eq!(deleted.revision, 4);
    assert!(deleted.contacts.is_empty());
    assert_eq!(directory.projection(), deleted);
}

#[tokio::test]
async fn a_replacement_directory_uses_a_distinct_projection_source() {
    let retired = Directory::new(Arc::new(FakeContactsStore::default()));
    for _ in 0..3 {
        retired.refresh().await.unwrap();
    }
    let delayed_retired_projection = retired.projection();

    let replacement = Directory::new(Arc::new(FakeContactsStore::default()));
    let replacement_projection = replacement.projection();

    assert_ne!(delayed_retired_projection.source_id, replacement_projection.source_id);
    assert!(delayed_retired_projection.revision > replacement_projection.revision);
}

fn valid_public_key() -> String {
    LocalPeerIdentity::generate().public_key_base64()
}

#[tokio::test]
async fn update_refreshes_the_cached_contact_from_the_store() {
    let directory = Directory::new(Arc::new(FakeContactsStore::with_contacts(vec![model(
        7, "peer-old", "Old Name",
    )])));
    directory.refresh().await.unwrap();
    let created_at = directory.contacts()[0].created_at;

    let new_key = valid_public_key();
    directory.update(7, "peer-new".into(), "New Name".into(), new_key.clone()).await.unwrap();

    let contacts = directory.contacts();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].peer_id.as_str(), "peer-new");
    assert_eq!(contacts[0].name, "New Name");
    assert_eq!(contacts[0].trusted_public_key, new_key);
    assert_eq!(contacts[0].created_at, created_at);
}

#[tokio::test]
async fn create_rejects_invalid_identity_fields_before_persistence() {
    let directory = Directory::new(Arc::new(FakeContactsStore::default()));

    assert!(directory.create(" ".into(), "Alice".into(), valid_public_key()).await.is_err());
    assert!(directory.create("peer-a".into(), " ".into(), valid_public_key()).await.is_err());
    assert!(directory.create("peer-a".into(), "Alice".into(), "not-a-key".into()).await.is_err());
    assert!(directory.contacts().is_empty());
}

#[tokio::test]
async fn stale_refresh_cannot_resurrect_a_deleted_trusted_contact() {
    let store = Arc::new(BlockingListStore::with_contacts(vec![model(7, "peer-a", "Alice")]));
    let directory = Directory::new(store.clone());
    directory.refresh().await.unwrap();

    store.block_next_list.store(true, Ordering::SeqCst);
    let refresh_directory = directory.clone();
    let refresh = tokio::spawn(async move { refresh_directory.refresh().await });
    store.list_started.notified().await;

    let delete_directory = directory.clone();
    let delete = tokio::spawn(async move { delete_directory.delete(7).await });
    tokio::task::yield_now().await;
    store.release_list.notify_one();

    refresh.await.unwrap().unwrap();
    delete.await.unwrap().unwrap();
    assert!(directory.contacts().is_empty());
}

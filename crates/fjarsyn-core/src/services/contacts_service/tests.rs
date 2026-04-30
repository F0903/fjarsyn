use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, Ordering},
};

use async_trait::async_trait;
use chrono::Utc;

use super::{Contact, ContactsService};
use crate::{database::ContactModel, repositories::ContactsStore};

#[derive(Default)]
struct FakeContactsStore {
    next_id: AtomicI64,
    contacts: Mutex<Vec<ContactModel>>,
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

    async fn get_by_id(&self, id: i64) -> Result<Option<ContactModel>, crate::Error> {
        Ok(self.contacts.lock().unwrap().iter().find(|contact| contact.id == id).cloned())
    }

    async fn create(
        &self,
        peer_id: String,
        name: String,
        address: Option<String>,
        trusted_public_key: Option<String>,
    ) -> Result<i64, crate::Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.contacts.lock().unwrap().insert(
            0,
            ContactModel { id, peer_id, name, address, trusted_public_key, created_at: Utc::now() },
        );
        Ok(id)
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
        address: Option<String>,
        trusted_public_key: Option<String>,
    ) -> Result<(), crate::Error> {
        if let Some(contact) =
            self.contacts.lock().unwrap().iter_mut().find(|contact| contact.id == id)
        {
            contact.peer_id = peer_id;
            contact.name = name;
            contact.address = address;
            contact.trusted_public_key = trusted_public_key;
        }
        Ok(())
    }
}

fn model(id: i64, peer_id: &str, name: &str) -> ContactModel {
    ContactModel {
        id,
        peer_id: peer_id.to_string(),
        name: name.to_string(),
        address: Some("127.0.0.1:9000".into()),
        trusted_public_key: Some(format!("trusted-key-{peer_id}")),
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn refresh_uses_fake_store() {
    let service = ContactsService::new(Arc::new(FakeContactsStore::with_contacts(vec![
        model(2, "peer-b", "B"),
        model(1, "peer-a", "A"),
    ])));

    service.refresh().await.unwrap();

    assert_eq!(
        service.contacts().as_ref(),
        &vec![
            Contact {
                id: 2,
                peer_id: "peer-b".into(),
                name: "B".into(),
                address: Some("127.0.0.1:9000".into()),
                trusted_public_key: Some("trusted-key-peer-b".into()),
            },
            Contact {
                id: 1,
                peer_id: "peer-a".into(),
                name: "A".into(),
                address: Some("127.0.0.1:9000".into()),
                trusted_public_key: Some("trusted-key-peer-a".into()),
            },
        ]
    );
}

#[tokio::test]
async fn create_updates_cache_without_database_pool() {
    let service = ContactsService::new(Arc::new(FakeContactsStore::default()));

    let id = service
        .create("peer-new".into(), "New Contact".into(), Some("10.0.0.1:9999".into()), None)
        .await
        .unwrap();

    let contacts = service.contacts();
    assert_eq!(id, contacts[0].id);
    assert_eq!(contacts[0].peer_id, "peer-new");
    assert_eq!(contacts[0].name, "New Contact");
}

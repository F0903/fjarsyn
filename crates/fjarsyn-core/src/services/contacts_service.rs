use std::sync::{Arc, RwLock};

use crate::{Error, repositories::ContactsStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: i64,
    pub peer_id: String,
    pub name: String,
    pub address: Option<String>,
}

#[derive(Clone)]
pub struct ContactsService {
    repository: Arc<dyn ContactsStore>,
    cache: Arc<RwLock<Arc<Vec<Contact>>>>,
}

impl ContactsService {
    pub fn new(repository: Arc<dyn ContactsStore>) -> Self {
        Self { repository, cache: Arc::new(RwLock::new(Arc::new(Vec::new()))) }
    }

    pub fn contacts(&self) -> Arc<Vec<Contact>> {
        self.cache.read().unwrap().clone()
    }

    pub async fn refresh(&self) -> Result<(), Arc<Error>> {
        let models = self.repository.list().await.map_err(Arc::new)?;
        let contacts = models
            .into_iter()
            .map(|m| Contact { id: m.id, peer_id: m.peer_id, name: m.name, address: m.address })
            .collect();
        let mut lock = self.cache.write().unwrap();
        *lock = Arc::new(contacts);
        Ok(())
    }

    pub async fn create(
        &self,
        peer_id: String,
        name: String,
        address: Option<String>,
    ) -> Result<i64, Arc<Error>> {
        let id = self.repository.create(peer_id, name, address).await.map_err(Arc::new)?;
        let contact_model = self.repository.get_by_id(id).await.ok().flatten();

        if let Some(m) = contact_model {
            let contact =
                Contact { id: m.id, peer_id: m.peer_id, name: m.name, address: m.address };
            let mut lock = self.cache.write().unwrap();
            let mut new_vec = (**lock).clone();
            new_vec.insert(0, contact);
            *lock = Arc::new(new_vec);
        } else {
            self.refresh().await?;
        }

        Ok(id)
    }

    pub async fn delete(&self, id: i64) -> Result<(), Arc<Error>> {
        self.repository.delete(id).await.map_err(Arc::new)?;

        {
            let mut lock = self.cache.write().unwrap();
            let mut new_vec = (**lock).clone();
            new_vec.retain(|c| c.id != id);
            *lock = Arc::new(new_vec);
        }

        Ok(())
    }

    pub async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        address: Option<String>,
    ) -> Result<(), Arc<Error>> {
        self.repository
            .update(id, peer_id.clone(), name.clone(), address.clone())
            .await
            .map_err(Arc::new)?;

        let needs_refresh = {
            let mut lock = self.cache.write().unwrap();
            let mut new_vec = (**lock).clone();
            if let Some(contact) = new_vec.iter_mut().find(|c| c.id == id) {
                contact.peer_id = peer_id;
                contact.name = name;
                contact.address = address;
                *lock = Arc::new(new_vec);
                false
            } else {
                true
            }
        };

        if needs_refresh {
            self.refresh().await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
        ) -> Result<i64, crate::Error> {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            self.contacts
                .lock()
                .unwrap()
                .insert(0, ContactModel { id, peer_id, name, address, created_at: Utc::now() });
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
        ) -> Result<(), crate::Error> {
            if let Some(contact) =
                self.contacts.lock().unwrap().iter_mut().find(|contact| contact.id == id)
            {
                contact.peer_id = peer_id;
                contact.name = name;
                contact.address = address;
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
                },
                Contact {
                    id: 1,
                    peer_id: "peer-a".into(),
                    name: "A".into(),
                    address: Some("127.0.0.1:9000".into()),
                },
            ]
        );
    }

    #[tokio::test]
    async fn create_updates_cache_without_database_pool() {
        let service = ContactsService::new(Arc::new(FakeContactsStore::default()));

        let id = service
            .create("peer-new".into(), "New Contact".into(), Some("10.0.0.1:9999".into()))
            .await
            .unwrap();

        let contacts = service.contacts();
        assert_eq!(id, contacts[0].id);
        assert_eq!(contacts[0].peer_id, "peer-new");
        assert_eq!(contacts[0].name, "New Contact");
    }
}

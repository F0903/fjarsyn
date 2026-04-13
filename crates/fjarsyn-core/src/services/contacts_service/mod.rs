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
mod tests;

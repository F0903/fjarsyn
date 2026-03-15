use std::sync::{Arc, RwLock};

use sqlx::SqlitePool;

use crate::{Error, database::ContactModel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: i64,
    pub peer_id: String,
    pub name: String,
    pub address: Option<String>,
}

#[derive(Clone)]
pub struct ContactsService {
    db: SqlitePool,
    cache: Arc<RwLock<Arc<Vec<Contact>>>>,
}

impl ContactsService {
    pub fn new(db: SqlitePool) -> Self {
        Self { db, cache: Arc::new(RwLock::new(Arc::new(Vec::new()))) }
    }

    /// Returns a read-only reference to the cached contacts.
    pub fn contacts(&self) -> Arc<Vec<Contact>> {
        self.cache.read().unwrap().clone()
    }

    /// Refreshes the internal cache from the database.
    pub async fn refresh(&self) -> Result<(), Arc<Error>> {
        let models = ContactModel::list(&self.db).await.map_err(Arc::new)?;
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
        let id = ContactModel::create(&self.db, peer_id, name, address).await.map_err(Arc::new)?;

        let contact_model = ContactModel::get_by_id(&self.db, id).await.ok().flatten();

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
        ContactModel::delete(&self.db, id).await.map_err(Arc::new)?;

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
        ContactModel::update(&self.db, id, peer_id.clone(), name.clone(), address.clone())
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

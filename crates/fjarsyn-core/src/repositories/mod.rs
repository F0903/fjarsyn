pub mod contacts_repository;
pub mod messages_repository;

use async_trait::async_trait;
pub use contacts_repository::ContactsRepository;
pub use messages_repository::MessagesRepository;

#[async_trait]
pub trait ContactsStore: Send + Sync {
    async fn list(&self) -> Result<Vec<crate::database::ContactModel>, crate::Error>;
    async fn get_by_id(
        &self,
        id: i64,
    ) -> Result<Option<crate::database::ContactModel>, crate::Error>;
    async fn create(
        &self,
        peer_id: String,
        name: String,
        address: Option<String>,
    ) -> Result<i64, crate::Error>;
    async fn delete(&self, id: i64) -> Result<(), crate::Error>;
    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        address: Option<String>,
    ) -> Result<(), crate::Error>;
}

#[async_trait]
pub trait MessagesStore: Send + Sync {
    async fn list(&self) -> Result<Vec<crate::database::MessageModel>, crate::Error>;
    async fn get_by_id(
        &self,
        id: i64,
    ) -> Result<Option<crate::database::MessageModel>, crate::Error>;
    async fn get_by_message_id_and_direction(
        &self,
        message_id: String,
        direction: String,
    ) -> Result<Option<crate::database::MessageModel>, crate::Error>;
    async fn create_outgoing(
        &self,
        message_id: String,
        peer_id: String,
        body: String,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, crate::Error>;
    async fn create_incoming_if_missing(
        &self,
        message_id: String,
        peer_id: String,
        body: String,
        created_at: chrono::DateTime<chrono::Utc>,
        delivered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, crate::Error>;
    async fn mark_delivered(
        &self,
        message_id: String,
        delivered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, crate::Error>;
    async fn mark_failed(&self, message_id: String) -> Result<bool, crate::Error>;
}

pub mod contacts_repository;
pub mod messages_repository;

use async_trait::async_trait;
pub use contacts_repository::ContactsRepository;
pub use messages_repository::MessagesRepository;

#[async_trait]
pub trait ContactsStore: Send + Sync {
    async fn list(&self) -> Result<Vec<crate::database::ContactModel>, crate::Error>;
    async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<crate::database::ContactModel, crate::Error>;
    async fn delete(&self, id: i64) -> Result<(), crate::Error>;
    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<crate::database::ContactModel, crate::Error>;
}

#[async_trait]
pub trait MessagesStore: Send + Sync {
    async fn list(&self) -> Result<Vec<crate::database::MessageModel>, crate::Error>;
    async fn create_outgoing(
        &self,
        session_id: crate::peer_session::SessionId,
        message_id: crate::peer_session::MessageId,
        peer_id: crate::peer_session::PeerId,
        body: String,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<crate::database::MessageModel, crate::Error>;
    async fn create_incoming_if_missing(
        &self,
        session_id: crate::peer_session::SessionId,
        message_id: crate::peer_session::MessageId,
        peer_id: crate::peer_session::PeerId,
        body: String,
        created_at: chrono::DateTime<chrono::Utc>,
        received_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<crate::database::MessageModel>, crate::Error>;
    async fn mark_sent(
        &self,
        session_id: crate::peer_session::SessionId,
        peer_id: crate::peer_session::PeerId,
        message_id: crate::peer_session::MessageId,
    ) -> Result<Option<crate::database::MessageModel>, crate::Error>;
    async fn mark_delivered(
        &self,
        session_id: crate::peer_session::SessionId,
        peer_id: crate::peer_session::PeerId,
        message_id: crate::peer_session::MessageId,
        delivered_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<crate::database::MessageModel>, crate::Error>;
    async fn mark_failed(
        &self,
        session_id: crate::peer_session::SessionId,
        peer_id: crate::peer_session::PeerId,
        message_id: crate::peer_session::MessageId,
    ) -> Result<Option<crate::database::MessageModel>, crate::Error>;
    async fn mark_unknown(
        &self,
        session_id: crate::peer_session::SessionId,
        peer_id: crate::peer_session::PeerId,
        message_id: crate::peer_session::MessageId,
    ) -> Result<Option<crate::database::MessageModel>, crate::Error>;
    async fn mark_all_pending_unknown(
        &self,
    ) -> Result<Vec<crate::database::MessageModel>, crate::Error>;
}

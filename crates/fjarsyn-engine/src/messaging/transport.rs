//! Messaging transport boundary and peer-session adapter.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::peer_session::{self, MessageId, SessionId};

#[async_trait]
pub(in crate::messaging) trait SessionMessaging: Send + Sync {
    fn snapshot(&self) -> peer_session::Sessions;

    async fn send_message(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    ) -> Result<(), peer_session::Error>;

    async fn send_receipt(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), peer_session::Error>;
}

#[async_trait]
impl SessionMessaging for peer_session::ServiceHandle {
    fn snapshot(&self) -> peer_session::Sessions {
        peer_session::ServiceHandle::snapshot(self)
    }

    async fn send_message(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    ) -> Result<(), peer_session::Error> {
        peer_session::ServiceHandle::send_message(self, session_id, message_id, body, sent_at).await
    }

    async fn send_receipt(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), peer_session::Error> {
        peer_session::ServiceHandle::send_receipt(self, session_id, message_id, received_at).await
    }
}

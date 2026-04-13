mod cache;
mod routing;
mod signals;

#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use chrono::Utc;
use tokio::sync::{Mutex, mpsc};

pub use crate::communication::messaging::{
    ConversationMessage, ConversationSummary, MessageDirection, MessageStatus, MessagingEvent,
};
use crate::{
    Error as CoreError,
    communication::messaging::{ConversationMap, MessageRecordError as CoreMessageRecordError},
    networking::{protocol::SignalingMessage, webrtc::WebRTC},
    repositories::MessagesStore,
};

type ConversationCache = Arc<RwLock<ConversationMap>>;
type SummaryCache = Arc<RwLock<Arc<Vec<ConversationSummary>>>>;
type DirectRouteMap = Arc<Mutex<HashMap<String, mpsc::Sender<SignalingMessage>>>>;

pub struct MessagingServiceConfig {
    pub repository: Arc<dyn MessagesStore>,
    pub webrtc: Arc<WebRTC>,
    pub event_tx: mpsc::Sender<MessagingEvent>,
}

#[derive(thiserror::Error, Debug)]
pub enum MessagingError {
    #[error("Database error: {0}")]
    Database(#[from] CoreError),
    #[error("Signaling error: {0}")]
    Signaling(#[from] crate::networking::signaling_error::SignalingError),
    #[error("Protocol error: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error(transparent)]
    MessageRecord(#[from] CoreMessageRecordError),
    #[error("Signal dispatch failed")]
    SignalDispatchFailed,
    #[error("No route available for peer {0}")]
    RouteUnavailable(String),
    #[error("Message body cannot be empty")]
    EmptyBody,
    #[error("Invalid message record: {0}")]
    InvalidMessageRecord(String),
}

pub struct MessagingService {
    repository: Arc<dyn MessagesStore>,
    webrtc: Arc<WebRTC>,
    conversations: ConversationCache,
    summaries: SummaryCache,
    direct_routes: DirectRouteMap,
    event_tx: mpsc::Sender<MessagingEvent>,
    signal_task: Option<tokio::task::JoinHandle<()>>,
}

impl std::fmt::Debug for MessagingService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessagingService").finish_non_exhaustive()
    }
}

impl MessagingService {
    pub async fn init(config: MessagingServiceConfig) -> Result<Self, MessagingError> {
        let (signal_tx, signal_rx) = mpsc::channel(100);
        config.webrtc.register_message_signal_sink(signal_tx).await;

        let mut service = Self {
            repository: config.repository,
            webrtc: config.webrtc,
            conversations: Arc::new(RwLock::new(HashMap::new())),
            summaries: Arc::new(RwLock::new(Arc::new(Vec::new()))),
            direct_routes: Arc::new(Mutex::new(HashMap::new())),
            event_tx: config.event_tx,
            signal_task: None,
        };

        service.refresh().await?;

        let task = Self::spawn_signal_task(
            service.repository.clone(),
            service.webrtc.clone(),
            service.conversations.clone(),
            service.summaries.clone(),
            service.direct_routes.clone(),
            service.event_tx.clone(),
            signal_rx,
        );
        service.signal_task = Some(task);

        Ok(service)
    }

    pub fn conversation_summaries(&self) -> Arc<Vec<ConversationSummary>> {
        self.summaries.read().unwrap().clone()
    }

    pub fn messages_for_peer(&self, peer_id: &str) -> Arc<Vec<ConversationMessage>> {
        self.conversations
            .read()
            .unwrap()
            .get(peer_id)
            .cloned()
            .unwrap_or_else(|| Arc::new(Vec::new()))
    }

    pub async fn send_message(
        &self,
        peer_id: String,
        addr: SocketAddr,
        body: String,
    ) -> Result<String, MessagingError> {
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err(MessagingError::EmptyBody);
        }

        let message_id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now();

        let message_row_id = self
            .repository
            .create_outgoing(message_id.clone(), peer_id.clone(), body.clone(), created_at)
            .await
            .map_err(MessagingError::Database)?;

        let message = self
            .repository
            .get_by_id(message_row_id)
            .await
            .map_err(MessagingError::Database)?
            .ok_or_else(|| {
                MessagingError::InvalidMessageRecord(format!(
                    "Missing outgoing message row after insert: {}",
                    message_row_id
                ))
            })
            .and_then(|message| {
                ConversationMessage::try_from(message).map_err(MessagingError::from)
            })?;

        self.cache_message(message);
        self.dispatch_event(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() })
            .await?;

        let payload = crate::networking::protocol::ChatMessagePayload {
            message_id: message_id.clone(),
            body,
            sent_at: created_at,
        };
        let signal = self.build_chat_signal(&peer_id, payload)?;

        if let Err(err) = self.send_with_retry(&peer_id, addr, signal).await {
            if self
                .repository
                .mark_failed(message_id.clone())
                .await
                .map_err(MessagingError::Database)?
                && let Some(message) = self
                    .repository
                    .get_by_message_id_and_direction(message_id, "outgoing".to_string())
                    .await
                    .map_err(MessagingError::Database)?
            {
                self.cache_message(ConversationMessage::try_from(message)?);
            }
            let _ = self
                .dispatch_event(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() })
                .await;
            return Err(err);
        }

        Ok(peer_id)
    }

    pub async fn refresh(&self) -> Result<(), MessagingError> {
        let messages = Self::load_messages(&self.repository).await?;
        let (conversations, summaries) = Self::build_caches(messages);

        let mut conversations_lock = self.conversations.write().unwrap();
        *conversations_lock = conversations;

        let mut summaries_lock = self.summaries.write().unwrap();
        *summaries_lock = summaries;
        Ok(())
    }

    fn cache_message(&self, message: ConversationMessage) {
        Self::cache_message_snapshot(&self.conversations, &self.summaries, message);
    }
}

impl Drop for MessagingService {
    fn drop(&mut self) {
        if let Some(task) = self.signal_task.take() {
            tracing::debug!("Aborting MessagingService task...");
            task.abort();
        }
    }
}

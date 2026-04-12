use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use chrono::Utc;
use tokio::sync::{Mutex, mpsc};

pub use crate::messaging::{
    ConversationMessage, ConversationSummary, MessageDirection, MessageStatus, MessagingEvent,
};
use crate::{
    Error as CoreError,
    messaging::{
        ConversationMap, MessageRecordError as CoreMessageRecordError, build_conversation_caches,
        build_conversation_summaries, upsert_conversation_message,
    },
    networking::{
        protocol::{ChatMessagePayload, ChatReceiptPayload, SignalingMessage, SignalingType},
        signaling,
        signaling_error::SignalingError,
        webrtc::{MessagingSignalEvent, WebRTC},
    },
    repositories::MessagesStore,
};

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
    Signaling(#[from] SignalingError),
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
    conversations: Arc<RwLock<ConversationMap>>,
    summaries: Arc<RwLock<Arc<Vec<ConversationSummary>>>>,
    direct_routes: Arc<Mutex<HashMap<String, mpsc::Sender<SignalingMessage>>>>,
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

        let payload =
            ChatMessagePayload { message_id: message_id.clone(), body, sent_at: created_at };
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

    fn spawn_signal_task(
        repository: Arc<dyn MessagesStore>,
        webrtc: Arc<WebRTC>,
        conversations: Arc<RwLock<ConversationMap>>,
        summaries: Arc<RwLock<Arc<Vec<ConversationSummary>>>>,
        direct_routes: Arc<Mutex<HashMap<String, mpsc::Sender<SignalingMessage>>>>,
        event_tx: mpsc::Sender<MessagingEvent>,
        mut signal_rx: mpsc::Receiver<MessagingSignalEvent>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(event) = signal_rx.recv().await {
                if let Err(err) = Self::handle_signal_event(
                    &repository,
                    &webrtc,
                    &conversations,
                    &summaries,
                    &direct_routes,
                    &event_tx,
                    event,
                )
                .await
                {
                    tracing::error!("Messaging signal handling failed: {}", err);
                }
            }
        })
    }

    async fn handle_signal_event(
        repository: &Arc<dyn MessagesStore>,
        webrtc: &Arc<WebRTC>,
        conversations: &Arc<RwLock<ConversationMap>>,
        summaries: &Arc<RwLock<Arc<Vec<ConversationSummary>>>>,
        direct_routes: &Arc<Mutex<HashMap<String, mpsc::Sender<SignalingMessage>>>>,
        event_tx: &mpsc::Sender<MessagingEvent>,
        event: MessagingSignalEvent,
    ) -> Result<(), MessagingError> {
        match event {
            MessagingSignalEvent::IncomingMessage { from, payload } => {
                let received_at = Utc::now();

                let inserted = repository
                    .create_incoming_if_missing(
                        payload.message_id.clone(),
                        from.clone(),
                        payload.body.clone(),
                        payload.sent_at,
                        received_at,
                    )
                    .await
                    .map_err(MessagingError::Database)?;

                if inserted
                    && let Some(message) = repository
                        .get_by_message_id_and_direction(
                            payload.message_id.clone(),
                            "incoming".to_string(),
                        )
                        .await
                        .map_err(MessagingError::Database)?
                {
                    Self::cache_message_snapshot(
                        conversations,
                        summaries,
                        ConversationMessage::try_from(message)?,
                    );
                    Self::send_event(
                        event_tx,
                        MessagingEvent::ConversationUpdated { peer_id: from.clone() },
                    )
                    .await?;
                    Self::send_event(
                        event_tx,
                        MessagingEvent::IncomingMessage {
                            peer_id: from.clone(),
                            body: payload.body.clone(),
                        },
                    )
                    .await?;
                }

                Self::send_receipt(
                    webrtc,
                    direct_routes,
                    from,
                    ChatReceiptPayload { message_id: payload.message_id, received_at },
                )
                .await?;
            }
            MessagingSignalEvent::Receipt { from, payload } => {
                let message_id = payload.message_id.clone();
                let changed = repository
                    .mark_delivered(message_id.clone(), payload.received_at)
                    .await
                    .map_err(MessagingError::Database)?;

                if changed
                    && let Some(message) = repository
                        .get_by_message_id_and_direction(message_id, "outgoing".to_string())
                        .await
                        .map_err(MessagingError::Database)?
                {
                    Self::cache_message_snapshot(
                        conversations,
                        summaries,
                        ConversationMessage::try_from(message)?,
                    );
                    Self::send_event(
                        event_tx,
                        MessagingEvent::ConversationUpdated { peer_id: from },
                    )
                    .await?;
                }
            }
        }

        Ok(())
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

    async fn load_messages(
        repository: &Arc<dyn MessagesStore>,
    ) -> Result<Vec<ConversationMessage>, MessagingError> {
        repository
            .list()
            .await
            .map_err(MessagingError::Database)?
            .into_iter()
            .map(|message| ConversationMessage::try_from(message).map_err(MessagingError::from))
            .collect()
    }

    fn build_caches(
        messages: Vec<ConversationMessage>,
    ) -> (ConversationMap, Arc<Vec<ConversationSummary>>) {
        build_conversation_caches(messages)
    }

    fn build_summaries(conversations: &ConversationMap) -> Arc<Vec<ConversationSummary>> {
        build_conversation_summaries(conversations)
    }

    fn cache_message(&self, message: ConversationMessage) {
        Self::cache_message_snapshot(&self.conversations, &self.summaries, message);
    }

    fn cache_message_snapshot(
        conversations: &Arc<RwLock<ConversationMap>>,
        summaries: &Arc<RwLock<Arc<Vec<ConversationSummary>>>>,
        message: ConversationMessage,
    ) {
        let mut conversations_lock = conversations.write().unwrap();
        upsert_conversation_message(&mut conversations_lock, message);

        let new_summaries = Self::build_summaries(&conversations_lock);
        drop(conversations_lock);

        let mut summaries_lock = summaries.write().unwrap();
        *summaries_lock = new_summaries;
    }

    fn build_chat_signal(
        &self,
        peer_id: &str,
        payload: ChatMessagePayload,
    ) -> Result<SignalingMessage, MessagingError> {
        Ok(SignalingMessage {
            from: self.webrtc.local_peer_id.clone(),
            to: Some(peer_id.to_string()),
            sig_type: SignalingType::ChatMessage,
            data: serde_json::to_string(&payload)?,
        })
    }

    async fn send_with_retry(
        &self,
        peer_id: &str,
        addr: SocketAddr,
        signal: SignalingMessage,
    ) -> Result<(), MessagingError> {
        let sender = self.ensure_direct_route(peer_id, addr).await?;
        if sender.send(signal.clone()).await.is_ok() {
            return Ok(());
        }

        {
            let mut routes = self.direct_routes.lock().await;
            routes.remove(peer_id);
        }

        let sender = self.ensure_direct_route(peer_id, addr).await?;
        sender.send(signal).await.map_err(|_| MessagingError::RouteUnavailable(peer_id.to_string()))
    }

    async fn ensure_direct_route(
        &self,
        peer_id: &str,
        addr: SocketAddr,
    ) -> Result<mpsc::Sender<SignalingMessage>, MessagingError> {
        if let Some(route) = self.direct_routes.lock().await.get(peer_id).cloned() {
            return Ok(route);
        }

        let route = signaling::dial(addr, self.webrtc.internal_signal_tx.clone()).await?;

        let mut routes = self.direct_routes.lock().await;
        routes.insert(peer_id.to_string(), route.clone());
        Ok(route)
    }

    async fn send_receipt(
        webrtc: &Arc<WebRTC>,
        direct_routes: &Arc<Mutex<HashMap<String, mpsc::Sender<SignalingMessage>>>>,
        peer_id: String,
        payload: ChatReceiptPayload,
    ) -> Result<(), MessagingError> {
        let signal = SignalingMessage {
            from: webrtc.local_peer_id.clone(),
            to: Some(peer_id.clone()),
            sig_type: SignalingType::ChatReceipt,
            data: serde_json::to_string(&payload)?,
        };

        if let Some(route) = direct_routes.lock().await.get(&peer_id).cloned()
            && route.send(signal.clone()).await.is_ok()
        {
            return Ok(());
        }

        webrtc
            .base_signaling_tx
            .send(signal)
            .await
            .map_err(|_| MessagingError::SignalDispatchFailed)
    }

    async fn dispatch_event(&self, event: MessagingEvent) -> Result<(), MessagingError> {
        Self::send_event(&self.event_tx, event).await
    }

    async fn send_event(
        event_tx: &mpsc::Sender<MessagingEvent>,
        event: MessagingEvent,
    ) -> Result<(), MessagingError> {
        event_tx.send(event).await.map_err(|_| MessagingError::SignalDispatchFailed)
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, RwLock},
    };

    use async_trait::async_trait;
    use chrono::{Duration, TimeZone};

    use super::{
        ConversationMessage, ConversationSummary, MessageDirection, MessageStatus, MessagingService,
    };
    use crate::{database::MessageModel, repositories::MessagesStore};

    #[derive(Default)]
    struct FakeMessagesStore {
        messages: Vec<MessageModel>,
    }

    #[async_trait]
    impl MessagesStore for FakeMessagesStore {
        async fn list(&self) -> Result<Vec<MessageModel>, crate::Error> {
            Ok(self.messages.clone())
        }

        async fn get_by_id(&self, id: i64) -> Result<Option<MessageModel>, crate::Error> {
            Ok(self.messages.iter().find(|message| message.id == id).cloned())
        }

        async fn get_by_message_id_and_direction(
            &self,
            message_id: String,
            direction: String,
        ) -> Result<Option<MessageModel>, crate::Error> {
            Ok(self
                .messages
                .iter()
                .find(|message| message.message_id == message_id && message.direction == direction)
                .cloned())
        }

        async fn create_outgoing(
            &self,
            _message_id: String,
            _peer_id: String,
            _body: String,
            _created_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<i64, crate::Error> {
            unreachable!("not needed in this test fake")
        }

        async fn create_incoming_if_missing(
            &self,
            _message_id: String,
            _peer_id: String,
            _body: String,
            _created_at: chrono::DateTime<chrono::Utc>,
            _delivered_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, crate::Error> {
            unreachable!("not needed in this test fake")
        }

        async fn mark_delivered(
            &self,
            _message_id: String,
            _delivered_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, crate::Error> {
            unreachable!("not needed in this test fake")
        }

        async fn mark_failed(&self, _message_id: String) -> Result<bool, crate::Error> {
            unreachable!("not needed in this test fake")
        }
    }

    fn message(id: i64, peer_id: &str, minutes: i64, body: &str) -> ConversationMessage {
        ConversationMessage {
            id,
            message_id: format!("message-{id}"),
            peer_id: peer_id.to_string(),
            direction: MessageDirection::Outgoing,
            body: body.to_string(),
            status: MessageStatus::Delivered,
            created_at: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()
                + Duration::minutes(minutes),
            delivered_at: None,
        }
    }

    #[test]
    fn build_summaries_orders_by_latest_message() {
        let mut conversations = HashMap::new();
        conversations.insert("peer-a".into(), Arc::new(vec![message(1, "peer-a", 1, "older")]));
        conversations.insert("peer-b".into(), Arc::new(vec![message(2, "peer-b", 5, "newer")]));

        let summaries = MessagingService::build_summaries(&conversations);

        assert_eq!(
            summaries.iter().map(|summary| summary.peer_id.as_str()).collect::<Vec<_>>(),
            vec!["peer-b", "peer-a"]
        );
    }

    #[test]
    fn cache_message_updates_one_thread_and_summary() {
        let conversations = Arc::new(RwLock::new(HashMap::from([(
            "peer-a".to_string(),
            Arc::new(vec![message(1, "peer-a", 1, "first")]),
        )])));
        let summaries = Arc::new(RwLock::new(Arc::new(vec![ConversationSummary::from(
            conversations.read().unwrap()["peer-a"].last().unwrap(),
        )])));

        MessagingService::cache_message_snapshot(
            &conversations,
            &summaries,
            message(2, "peer-a", 2, "second"),
        );

        let cached_messages = conversations.read().unwrap()["peer-a"].clone();
        assert_eq!(cached_messages.len(), 2);
        assert_eq!(cached_messages.last().unwrap().body, "second");
        assert_eq!(summaries.read().unwrap().first().unwrap().last_message_body, "second");
    }

    #[tokio::test]
    async fn load_messages_works_with_fake_store() {
        let created_at = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let repository: Arc<dyn MessagesStore> = Arc::new(FakeMessagesStore {
            messages: vec![MessageModel {
                id: 1,
                message_id: "msg-1".into(),
                peer_id: "peer-a".into(),
                direction: "incoming".into(),
                body: "hello".into(),
                status: "delivered".into(),
                created_at,
                delivered_at: Some(created_at),
            }],
        });

        let messages = MessagingService::load_messages(&repository).await.unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].peer_id, "peer-a");
        assert_eq!(messages[0].body, "hello");
        assert_eq!(messages[0].direction, MessageDirection::Incoming);
    }
}

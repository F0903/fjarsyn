use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use tokio::sync::{Mutex, mpsc};

use crate::{
    database::MessageModel,
    networking::{
        protocol::{ChatMessagePayload, ChatReceiptPayload, SignalingMessage, SignalingType},
        signaling,
        signaling_error::SignalingError,
        webrtc::{MessagingSignalEvent, WebRTC},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    Incoming,
    Outgoing,
}

impl MessageDirection {
    fn from_db(value: &str) -> Result<Self, MessagingError> {
        match value {
            "incoming" => Ok(Self::Incoming),
            "outgoing" => Ok(Self::Outgoing),
            _ => Err(MessagingError::InvalidMessageRecord(format!(
                "Unknown message direction: {}",
                value
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStatus {
    Pending,
    Delivered,
    Failed,
}

impl MessageStatus {
    fn from_db(value: &str) -> Result<Self, MessagingError> {
        match value {
            "pending" => Ok(Self::Pending),
            "delivered" => Ok(Self::Delivered),
            "failed" => Ok(Self::Failed),
            _ => Err(MessagingError::InvalidMessageRecord(format!(
                "Unknown message status: {}",
                value
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationMessage {
    pub id: i64,
    pub message_id: String,
    pub peer_id: String,
    pub direction: MessageDirection,
    pub body: String,
    pub status: MessageStatus,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

impl TryFrom<MessageModel> for ConversationMessage {
    type Error = MessagingError;

    fn try_from(value: MessageModel) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            message_id: value.message_id,
            peer_id: value.peer_id,
            direction: MessageDirection::from_db(&value.direction)?,
            body: value.body,
            status: MessageStatus::from_db(&value.status)?,
            created_at: value.created_at,
            delivered_at: value.delivered_at,
        })
    }
}

#[derive(Debug, Clone)]
pub enum MessagingEvent {
    ConversationUpdated { peer_id: String },
    IncomingMessage { peer_id: String, body: String },
}

pub struct MessagingServiceConfig {
    pub db: SqlitePool,
    pub webrtc: Arc<WebRTC>,
    pub event_tx: mpsc::Sender<MessagingEvent>,
}

#[derive(thiserror::Error, Debug)]
pub enum MessagingError {
    #[error("Database error: {0}")]
    Database(#[from] crate::Error),
    #[error("Signaling error: {0}")]
    Signaling(#[from] SignalingError),
    #[error("Protocol error: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("Signal dispatch failed")]
    SignalDispatchFailed,
    #[error("No route available for peer {0}")]
    RouteUnavailable(String),
    #[error("Message body cannot be empty")]
    EmptyBody,
    #[error("Invalid message record: {0}")]
    InvalidMessageRecord(String),
}

#[derive(Debug)]
pub struct MessagingService {
    db: SqlitePool,
    webrtc: Arc<WebRTC>,
    cache: Arc<RwLock<Arc<Vec<ConversationMessage>>>>,
    direct_routes: Arc<Mutex<HashMap<String, mpsc::Sender<SignalingMessage>>>>,
    event_tx: mpsc::Sender<MessagingEvent>,
    signal_task: Option<tokio::task::JoinHandle<()>>,
}

impl MessagingService {
    pub async fn init(config: MessagingServiceConfig) -> Result<Self, MessagingError> {
        let (signal_tx, signal_rx) = mpsc::channel(100);
        config.webrtc.register_message_signal_sink(signal_tx).await;

        let mut service = Self {
            db: config.db,
            webrtc: config.webrtc,
            cache: Arc::new(RwLock::new(Arc::new(Vec::new()))),
            direct_routes: Arc::new(Mutex::new(HashMap::new())),
            event_tx: config.event_tx,
            signal_task: None,
        };

        service.refresh().await?;

        let task = Self::spawn_signal_task(
            service.db.clone(),
            service.webrtc.clone(),
            service.cache.clone(),
            service.direct_routes.clone(),
            service.event_tx.clone(),
            signal_rx,
        );
        service.signal_task = Some(task);

        Ok(service)
    }

    pub fn messages(&self) -> Arc<Vec<ConversationMessage>> {
        self.cache.read().unwrap().clone()
    }

    pub async fn refresh(&self) -> Result<(), MessagingError> {
        let messages = Self::load_messages(&self.db).await?;
        let mut cache = self.cache.write().unwrap();
        *cache = Arc::new(messages);
        Ok(())
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

        MessageModel::create_outgoing(
            &self.db,
            message_id.clone(),
            peer_id.clone(),
            body.clone(),
            created_at,
        )
        .await
        .map_err(MessagingError::Database)?;

        self.refresh().await?;
        self.dispatch_event(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() })
            .await?;

        let payload =
            ChatMessagePayload { message_id: message_id.clone(), body, sent_at: created_at };
        let signal = self.build_chat_signal(&peer_id, payload)?;

        if let Err(err) = self.send_with_retry(&peer_id, addr, signal).await {
            let _ = MessageModel::mark_failed(&self.db, message_id).await;
            self.refresh().await?;
            let _ = self
                .dispatch_event(MessagingEvent::ConversationUpdated { peer_id: peer_id.clone() })
                .await;
            return Err(err);
        }

        Ok(peer_id)
    }

    fn spawn_signal_task(
        db: SqlitePool,
        webrtc: Arc<WebRTC>,
        cache: Arc<RwLock<Arc<Vec<ConversationMessage>>>>,
        direct_routes: Arc<Mutex<HashMap<String, mpsc::Sender<SignalingMessage>>>>,
        event_tx: mpsc::Sender<MessagingEvent>,
        mut signal_rx: mpsc::Receiver<MessagingSignalEvent>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(event) = signal_rx.recv().await {
                if let Err(err) = Self::handle_signal_event(
                    &db,
                    &webrtc,
                    &cache,
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
        db: &SqlitePool,
        webrtc: &Arc<WebRTC>,
        cache: &Arc<RwLock<Arc<Vec<ConversationMessage>>>>,
        direct_routes: &Arc<Mutex<HashMap<String, mpsc::Sender<SignalingMessage>>>>,
        event_tx: &mpsc::Sender<MessagingEvent>,
        event: MessagingSignalEvent,
    ) -> Result<(), MessagingError> {
        match event {
            MessagingSignalEvent::IncomingMessage { from, payload } => {
                let received_at = Utc::now();

                let inserted = MessageModel::create_incoming_if_missing(
                    db,
                    payload.message_id.clone(),
                    from.clone(),
                    payload.body.clone(),
                    payload.sent_at,
                    received_at,
                )
                .await
                .map_err(MessagingError::Database)?;

                if inserted {
                    Self::refresh_cache(cache, db).await?;
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
                let changed =
                    MessageModel::mark_delivered(db, payload.message_id, payload.received_at)
                        .await
                        .map_err(MessagingError::Database)?;

                if changed {
                    Self::refresh_cache(cache, db).await?;
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

    async fn refresh_cache(
        cache: &Arc<RwLock<Arc<Vec<ConversationMessage>>>>,
        db: &SqlitePool,
    ) -> Result<(), MessagingError> {
        let messages = Self::load_messages(db).await?;
        let mut cache_lock = cache.write().unwrap();
        *cache_lock = Arc::new(messages);
        Ok(())
    }

    async fn load_messages(db: &SqlitePool) -> Result<Vec<ConversationMessage>, MessagingError> {
        MessageModel::list(db)
            .await
            .map_err(MessagingError::Database)?
            .into_iter()
            .map(ConversationMessage::try_from)
            .collect()
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

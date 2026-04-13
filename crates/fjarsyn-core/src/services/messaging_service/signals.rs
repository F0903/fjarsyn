use chrono::Utc;
use tokio::sync::mpsc;

use super::{
    ConversationCache, DirectRouteMap, MessagingError, MessagingEvent, MessagingService,
    SummaryCache,
};
use crate::{
    communication::messaging::ConversationMessage,
    networking::{protocol::ChatReceiptPayload, webrtc::MessagingSignalEvent},
    repositories::MessagesStore,
};

impl MessagingService {
    pub(super) fn spawn_signal_task(
        repository: std::sync::Arc<dyn MessagesStore>,
        webrtc: std::sync::Arc<crate::networking::webrtc::WebRTC>,
        conversations: ConversationCache,
        summaries: SummaryCache,
        direct_routes: DirectRouteMap,
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
        repository: &std::sync::Arc<dyn MessagesStore>,
        webrtc: &std::sync::Arc<crate::networking::webrtc::WebRTC>,
        conversations: &ConversationCache,
        summaries: &SummaryCache,
        direct_routes: &DirectRouteMap,
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

    pub(super) async fn dispatch_event(&self, event: MessagingEvent) -> Result<(), MessagingError> {
        Self::send_event(&self.event_tx, event).await
    }

    pub(super) async fn send_event(
        event_tx: &mpsc::Sender<MessagingEvent>,
        event: MessagingEvent,
    ) -> Result<(), MessagingError> {
        event_tx.send(event).await.map_err(|_| MessagingError::SignalDispatchFailed)
    }
}

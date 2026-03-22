use tokio::sync::mpsc;

use super::{MessagingSignalEvent, WebRTC};

impl WebRTC {
    pub async fn register_message_signal_sink(&self, tx: mpsc::Sender<MessagingSignalEvent>) {
        *self.message_signal_tx.write().await = Some(tx);
    }

    pub(super) async fn forward_message_signal(&self, event: MessagingSignalEvent) {
        let tx = self.message_signal_tx.read().await.clone();
        if let Some(tx) = tx {
            let _ = tx.send(event).await;
        }
    }
}

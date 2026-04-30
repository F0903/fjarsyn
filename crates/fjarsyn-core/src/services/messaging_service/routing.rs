use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::sync::{Mutex, mpsc};

use super::{MessagingError, MessagingService};
use crate::networking::{
    protocol::{ChatMessagePayload, ChatReceiptPayload, SignalingMessage, SignalingType},
    signaling,
    webrtc::WebRTC,
};

impl MessagingService {
    pub(super) fn build_chat_signal(
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

    pub(super) async fn send_with_retry(
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

    pub(super) async fn ensure_direct_route(
        &self,
        peer_id: &str,
        addr: SocketAddr,
    ) -> Result<mpsc::Sender<SignalingMessage>, MessagingError> {
        if let Some(route) = self.direct_routes.lock().await.get(peer_id).cloned() {
            return Ok(route);
        }

        let route = signaling::dial(
            addr,
            self.webrtc.signaling_auth_context(),
            self.webrtc.internal_signal_tx.clone(),
        )
        .await?;

        let mut routes = self.direct_routes.lock().await;
        routes.insert(peer_id.to_string(), route.clone());
        Ok(route)
    }

    pub(super) async fn send_receipt(
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
}

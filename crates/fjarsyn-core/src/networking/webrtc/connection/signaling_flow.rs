use std::sync::Arc;

use tokio::sync::mpsc;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidateInit,
    peer_connection::{
        peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
};

use super::{WebRTC, WebRTCEvent};
use crate::networking::{
    protocol::{ChatMessagePayload, ChatReceiptPayload, SignalingMessage, SignalingType},
    signaling,
    webrtc::{WebRTCError, webrtc_error::WebRTCResult},
};

impl WebRTC {
    pub(super) fn spawn_signaling_reader(
        self: Arc<Self>,
        mut signal_rx: mpsc::Receiver<SignalingMessage>,
    ) {
        let tasks = Arc::clone(&self.tasks);
        let handle = tokio::spawn(async move {
            while let Some(message) = signal_rx.recv().await {
                if let Err(err) = self.handle_signaling_message(message).await {
                    tracing::error!("Error handling signaling message: {}", err);
                }
            }
        });
        tasks.lock().unwrap().push(handle);
    }

    async fn handle_signaling_message(&self, msg: SignalingMessage) -> WebRTCResult<()> {
        if !msg.targets_peer(&self.local_peer_id) {
            tracing::debug!(
                "Ignoring signaling message from {} addressed to {:?}",
                msg.from,
                msg.to
            );
            return Ok(());
        }

        match msg.sig_type {
            SignalingType::Offer => {
                if self.is_busy_for_incoming_offer().await {
                    tracing::info!(
                        "Rejecting unexpected offer from {} because the current session is busy.",
                        msg.from
                    );
                    self.reject_incoming_offer(msg.from).await;
                    return Ok(());
                }

                let pc = self.prepare_pc().await?;

                *self.remote_peer_id.write().await = Some(msg.from.clone());

                let sdp = RTCSessionDescription::offer(msg.data).map_err(WebRTCError::SdpError)?;
                pc.set_remote_description(sdp).await.map_err(WebRTCError::PeerConnectionError)?;

                let _ = self.event_tx.send(WebRTCEvent::IncomingCall(msg.from.clone())).await;
            }
            SignalingType::Answer => {
                let pc = self.peer_connection.read().await;
                let sdp = RTCSessionDescription::answer(msg.data).map_err(WebRTCError::SdpError)?;
                pc.set_remote_description(sdp).await.map_err(WebRTCError::PeerConnectionError)?;
            }
            SignalingType::Candidate => {
                let pc = self.peer_connection.read().await;
                let candidate: RTCIceCandidateInit =
                    serde_json::from_str(&msg.data).map_err(WebRTCError::DeserializeError)?;
                let _ = pc.add_ice_candidate(candidate).await;
            }
            SignalingType::Decline => {
                let pc = {
                    let lock = self.peer_connection.read().await;
                    lock.clone()
                };

                if pc.connection_state() == RTCPeerConnectionState::Closed {
                    self.reset_signaling_session_state().await;
                    return Ok(());
                }

                tracing::info!("Call declined by remote peer: {}", msg.from);
                pc.close().await.map_err(WebRTCError::PeerConnectionError)?;
                self.reset_signaling_session_state().await;
            }
            SignalingType::ChatMessage => {
                let payload: ChatMessagePayload =
                    serde_json::from_str(&msg.data).map_err(WebRTCError::DeserializeError)?;
                self.forward_message_signal(super::MessagingSignalEvent::IncomingMessage {
                    from: msg.from,
                    payload,
                })
                .await;
            }
            SignalingType::ChatReceipt => {
                let payload: ChatReceiptPayload =
                    serde_json::from_str(&msg.data).map_err(WebRTCError::DeserializeError)?;
                self.forward_message_signal(super::MessagingSignalEvent::Receipt {
                    from: msg.from,
                    payload,
                })
                .await;
            }
        }
        Ok(())
    }

    pub async fn dial_direct(&self, addr: std::net::SocketAddr) -> WebRTCResult<()> {
        let tx =
            signaling::dial(addr, self.signaling_auth.clone(), self.internal_signal_tx.clone())
                .await?;
        *self.signaling_tx.write().await = Some(tx);
        Ok(())
    }

    async fn restore_base_signaling(&self) {
        *self.signaling_tx.write().await = Some(self.base_signaling_tx.clone());
    }

    async fn reset_signaling_session_state(&self) {
        self.clear_control_channel().await;
        *self.remote_peer_id.write().await = None;
        self.restore_base_signaling().await;
    }

    async fn is_busy_for_incoming_offer(&self) -> bool {
        if self.remote_peer_id.read().await.is_some() {
            return true;
        }

        let pc = self.peer_connection.read().await;
        !matches!(
            pc.connection_state(),
            RTCPeerConnectionState::New
                | RTCPeerConnectionState::Closed
                | RTCPeerConnectionState::Failed
        )
    }

    async fn reject_incoming_offer(&self, peer_id: String) {
        let message = SignalingMessage {
            from: self.local_peer_id.clone(),
            to: Some(peer_id),
            sig_type: SignalingType::Decline,
            data: String::new(),
        };

        if let Err(err) = self.base_signaling_tx.send(message).await {
            tracing::debug!("Failed to reject incoming offer: {}", err);
        }
    }

    pub async fn reset_after_failed_dial(&self) {
        let pc = {
            let lock = self.peer_connection.read().await;
            lock.clone()
        };

        if pc.connection_state() != RTCPeerConnectionState::Closed
            && let Err(err) = pc.close().await
        {
            tracing::debug!("Failed to close peer connection after dial failure: {}", err);
        }

        self.reset_signaling_session_state().await;
    }

    pub async fn create_offer(&self) -> WebRTCResult<()> {
        let pc = self.prepare_pc().await?;
        self.ensure_control_channel(&pc).await?;

        let offer = pc.create_offer(None).await.map_err(WebRTCError::PeerConnectionError)?;
        let sdp = offer.sdp.clone();
        pc.set_local_description(offer).await.map_err(WebRTCError::PeerConnectionError)?;

        let message = SignalingMessage {
            from: self.local_peer_id.clone(),
            to: self.remote_peer_id.read().await.clone(),
            sig_type: SignalingType::Offer,
            data: sdp,
        };

        let tx = {
            let lock = self.signaling_tx.read().await;
            lock.clone()
        };
        if let Some(tx) = tx {
            tx.send(message).await.map_err(WebRTCError::SendError)?;
        }
        Ok(())
    }

    pub async fn accept_call(&self) -> WebRTCResult<()> {
        let pc = self.peer_connection.read().await;
        let answer = pc.create_answer(None).await.map_err(WebRTCError::PeerConnectionError)?;

        let answer_sdp = answer.sdp.clone();
        pc.set_local_description(answer).await.map_err(WebRTCError::PeerConnectionError)?;

        let response = SignalingMessage {
            from: self.local_peer_id.clone(),
            to: self.remote_peer_id.read().await.clone(),
            sig_type: SignalingType::Answer,
            data: answer_sdp,
        };

        let tx = {
            let lock = self.signaling_tx.read().await;
            lock.clone()
        };
        if let Some(tx) = tx {
            tx.send(response).await.map_err(WebRTCError::SendError)?;
        }
        Ok(())
    }

    pub async fn decline_call(&self) -> WebRTCResult<()> {
        let message = SignalingMessage {
            from: self.local_peer_id.clone(),
            to: self.remote_peer_id.read().await.clone(),
            sig_type: SignalingType::Decline,
            data: String::new(),
        };

        {
            let lock = self.signaling_tx.read().await;
            if let Some(tx) = lock.as_ref() {
                tx.send(message).await.map_err(WebRTCError::SendError)?;
            }
        }

        {
            let pc = self.peer_connection.read().await;
            pc.close().await.map_err(WebRTCError::PeerConnectionError)?;
        }
        self.reset_signaling_session_state().await;

        Ok(())
    }

    pub async fn disconnect(&self) -> WebRTCResult<()> {
        let message = SignalingMessage {
            from: self.local_peer_id.clone(),
            to: self.remote_peer_id.read().await.clone(),
            sig_type: SignalingType::Decline,
            data: String::new(),
        };

        {
            let lock = self.signaling_tx.read().await;
            if let Some(tx) = lock.as_ref() {
                let _ = tx.send(message).await;
            }
        }

        {
            let pc = self.peer_connection.read().await;
            pc.close().await.map_err(WebRTCError::PeerConnectionError)?;
        }
        self.reset_signaling_session_state().await;

        Ok(())
    }
}

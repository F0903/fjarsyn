use std::sync::{Arc, RwLock};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::networking::{
    signaling::auth::{StoredIdentityKeypair, TrustedPeerIdentity},
    webrtc::{WebRTC, WebRTCError},
};

mod dial;
mod state;

#[cfg(test)]
mod tests;

pub use dial::{DialResult, DialSuccess};
pub use state::{CallEvent, CallState, CallTarget, CallTransportEvent};

pub struct CallServiceConfig {
    /// Channel for receiving video frames from remote peers.
    pub frame_packet_tx: mpsc::Sender<Bytes>,
    /// Channel for receiving call events (incoming call, connected, ended).
    pub call_event_tx: mpsc::Sender<CallEvent>,
    /// Maximum latency for depacketization.
    pub max_depacket_latency: u16,
    /// Optional peer ID (generated if None).
    pub peer_id: Option<String>,
    /// Optional persisted signaling identity keypair (generated if None).
    pub identity_keypair: Option<StoredIdentityKeypair>,
}

/// Service managing calls. Owns WebRTC and handles all call-related logic internally.
#[derive(Debug)]
pub struct CallService {
    webrtc: Arc<WebRTC>,
    state: Arc<RwLock<CallState>>,
    event_task: Option<tokio::task::JoinHandle<()>>,
}

impl CallService {
    pub async fn init(config: CallServiceConfig) -> Result<Self, WebRTCError> {
        let (webrtc_event_tx, mut webrtc_event_rx) = mpsc::channel(100);
        let event_tx = config.call_event_tx;

        let webrtc = WebRTC::init(
            config.frame_packet_tx,
            webrtc_event_tx,
            config.max_depacket_latency,
            config.peer_id,
            config.identity_keypair,
        )
        .await?;

        let state = Arc::new(RwLock::new(CallState::default()));
        let state_clone = Arc::clone(&state);
        let event_tx_clone = event_tx.clone();

        let event_task = tokio::spawn(async move {
            while let Some(event) = webrtc_event_rx.recv().await {
                let ui_event = {
                    let mut state = state_clone.write().unwrap();
                    let event = state::map_webrtc_event(event);
                    state.apply_event(event)
                };

                if let Some(event) = ui_event {
                    let _ = event_tx_clone.send(event).await;
                }
            }
        });

        Ok(Self { webrtc, state, event_task: Some(event_task) })
    }

    pub fn state(&self) -> CallState {
        self.state.read().unwrap().clone()
    }

    pub fn is_in_call(&self) -> bool {
        matches!(*self.state.read().unwrap(), CallState::InCall { .. })
    }

    pub fn has_incoming_call(&self) -> bool {
        matches!(*self.state.read().unwrap(), CallState::IncomingCall { .. })
    }

    pub fn local_id(&self) -> &str {
        &self.webrtc.local_peer_id
    }

    pub fn local_public_key(&self) -> String {
        self.webrtc.local_public_key()
    }

    pub fn signaling_port(&self) -> u16 {
        self.webrtc.direct_signaling_port
    }

    // TODO: in the future we could abstract this further by just returning an abstract Writer or something similar.
    pub fn webrtc(&self) -> Arc<WebRTC> {
        Arc::clone(&self.webrtc)
    }

    pub fn replace_trusted_contacts<'a>(
        &self,
        contacts: impl IntoIterator<Item = &'a crate::services::contacts_service::Contact>,
    ) {
        self.webrtc.replace_trusted_peers(contacts.into_iter().filter_map(|contact| {
            contact.trusted_public_key.as_ref().map(|public_key| {
                TrustedPeerIdentity::new(contact.peer_id.clone(), public_key.clone())
            })
        }));
    }

    pub async fn accept(&self) -> Result<(), WebRTCError> {
        let peer_id = match &*self.state.read().unwrap() {
            CallState::IncomingCall { peer_id } => Some(peer_id.clone()),
            _ => None,
        };

        self.webrtc.accept_call().await?;
        *self.state.write().unwrap() = CallState::InCall { peer_id };
        Ok(())
    }

    pub async fn decline(&self) -> Result<(), WebRTCError> {
        let result = self.webrtc.decline_call().await;
        *self.state.write().unwrap() = CallState::Idle;
        result
    }

    pub async fn end(&self) -> Result<(), WebRTCError> {
        let result = self.webrtc.disconnect().await;
        *self.state.write().unwrap() = CallState::Idle;
        result
    }
}

impl Drop for CallService {
    fn drop(&mut self) {
        if let Some(task) = self.event_task.take() {
            tracing::debug!("Aborting CallService task...");
            task.abort();
        }
    }
}

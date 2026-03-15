use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{
    networking::{
        discovery::PeerInfo,
        webrtc::{WebRTC, WebRTCError, WebRTCEvent},
    },
    services::contacts_service::Contact,
    ui::message::CallTarget,
};

#[derive(Debug, Clone)]
pub enum CallEvent {
    IncomingCall { peer_id: String },
    CallConnected,
    CallEnded,
}

pub struct CallServiceConfig {
    /// Channel for receiving video frames from remote peers.
    pub frame_packet_tx: mpsc::Sender<Bytes>,
    /// Channel for receiving call events (incoming call, connected, ended).
    pub call_event_tx: mpsc::Sender<CallEvent>,
    /// Maximum latency for depacketization.
    pub max_depacket_latency: u16,
    /// Optional peer ID (generated if None).
    pub peer_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum CallState {
    #[default]
    Idle,
    Dialing {
        target: CallTarget,
    },
    IncomingCall {
        peer_id: String,
    },
    InCall {
        peer_id: Option<String>,
    },
}

impl CallState {
    pub fn apply_event(&mut self, event: WebRTCEvent) -> Option<CallEvent> {
        match event {
            WebRTCEvent::IncomingCall(peer_id) => {
                if matches!(*self, CallState::Idle) {
                    *self = CallState::IncomingCall { peer_id: peer_id.clone() };
                    Some(CallEvent::IncomingCall { peer_id })
                } else {
                    None
                }
            }
            WebRTCEvent::Connected => {
                let peer_id = match self {
                    CallState::Dialing { .. } => None,
                    CallState::IncomingCall { peer_id } => Some(peer_id.clone()),
                    CallState::InCall { peer_id } => peer_id.clone(),
                    _ => None,
                };
                *self = CallState::InCall { peer_id };
                Some(CallEvent::CallConnected)
            }
            WebRTCEvent::Disconnected => {
                *self = CallState::Idle;
                Some(CallEvent::CallEnded)
            }
        }
    }
}

/// Result of a successful dial operation.
pub struct DialSuccess {
    pub peer_id: Option<String>,
    pub name: Option<String>,
    pub socket_addr: Option<SocketAddr>,
    pub update_contact_address: Option<(i64, String)>,
}

pub type DialResult = Result<DialSuccess, String>;

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
        )
        .await?;

        let state = Arc::new(RwLock::new(CallState::default()));
        let state_clone = Arc::clone(&state);
        let event_tx_clone = event_tx.clone();

        // Spawn background task to process WebRTC events
        let event_task = tokio::spawn(async move {
            while let Some(event) = webrtc_event_rx.recv().await {
                let ui_event = {
                    let mut state = state_clone.write().unwrap();
                    state.apply_event(event)
                };

                if let Some(event) = ui_event {
                    let _ = event_tx_clone.send(event).await;
                }
            }
        });

        Ok(Self { webrtc, state, event_task: Some(event_task) })
    }

    /// Returns the current call state.
    pub fn state(&self) -> CallState {
        self.state.read().unwrap().clone()
    }

    pub fn is_in_call(&self) -> bool {
        matches!(*self.state.read().unwrap(), CallState::InCall { .. })
    }

    pub fn has_incoming_call(&self) -> bool {
        matches!(*self.state.read().unwrap(), CallState::IncomingCall { .. })
    }

    /// Returns our local peer ID.
    pub fn local_id(&self) -> &str {
        &self.webrtc.local_peer_id
    }

    /// Returns the direct signaling port.
    pub fn signaling_port(&self) -> u16 {
        self.webrtc.direct_signaling_port
    }

    //TODO: in the future we could abstract this further by just returning an abstract Writer or something similar.
    /// Returns a reference to the underlying WebRTC instance
    pub fn webrtc(&self) -> Arc<WebRTC> {
        Arc::clone(&self.webrtc)
    }

    pub async fn dial(
        &self,
        target: CallTarget,
        contacts: &[Contact],
        discovered: &[PeerInfo],
    ) -> DialResult {
        *self.state.write().unwrap() = CallState::Dialing { target: target.clone() };

        let ResolvedTarget { peer_id: tid, address: taddr, name: tname } =
            self.resolve_target(&target, contacts)?;

        // Try to connect via discovered peers
        if let Some(id) = &tid
            && let Some(p) = discovered.iter().find(|p| p.id == *id)
        {
            for addr in &p.addresses {
                let saddr = SocketAddr::new(*addr, p.port);
                if self.webrtc.dial_direct(saddr).await.is_ok() {
                    let mut update_contact_address = None;
                    if let CallTarget::ContactId(cid) = target {
                        let s = saddr.to_string();
                        if taddr.as_ref() != Some(&s) {
                            update_contact_address = Some((cid, s));
                        }
                    }

                    self.webrtc.create_offer().await.map_err(|e| format!("Offer failed: {}", e))?;

                    *self.state.write().unwrap() = CallState::InCall { peer_id: tid.clone() };

                    return Ok(DialSuccess {
                        peer_id: tid,
                        name: tname,
                        socket_addr: None,
                        update_contact_address,
                    });
                }
            }
        }

        // Try direct address connection
        if let Some(addr_str) = taddr {
            let addr = match addr_str.parse::<SocketAddr>() {
                Ok(addr) => addr,
                Err(_) => {
                    *self.state.write().unwrap() = CallState::Idle;
                    return Err("Invalid address format".into());
                }
            };

            if self.webrtc.dial_direct(addr).await.is_ok() {
                self.webrtc.create_offer().await.map_err(|e| format!("Offer failed: {}", e))?;

                *self.state.write().unwrap() = CallState::InCall { peer_id: tid.clone() };

                return Ok(DialSuccess {
                    peer_id: tid,
                    name: tname,
                    socket_addr: Some(addr),
                    update_contact_address: None,
                });
            }
        }

        *self.state.write().unwrap() = CallState::Idle;
        Err("Connection failed".into())
    }

    /// Accepts an incoming call.
    pub async fn accept(&self) -> Result<(), WebRTCError> {
        let peer_id = match &*self.state.read().unwrap() {
            CallState::IncomingCall { peer_id } => Some(peer_id.clone()),
            _ => None,
        };

        self.webrtc.accept_call().await?;
        *self.state.write().unwrap() = CallState::InCall { peer_id };
        Ok(())
    }

    /// Declines an incoming call.
    pub async fn decline(&self) -> Result<(), WebRTCError> {
        let result = self.webrtc.decline_call().await;
        *self.state.write().unwrap() = CallState::Idle;
        result
    }

    /// Ends the current call.
    pub async fn end(&self) -> Result<(), WebRTCError> {
        let result = self.webrtc.disconnect().await;
        *self.state.write().unwrap() = CallState::Idle;
        result
    }

    fn resolve_target(
        &self,
        target: &CallTarget,
        contacts: &[Contact],
    ) -> Result<ResolvedTarget, String> {
        match target {
            CallTarget::ContactId(id) => contacts
                .iter()
                .find(|c| c.id == *id)
                .map(|c| ResolvedTarget {
                    peer_id: Some(c.peer_id.clone()),
                    address: c.address.clone(),
                    name: Some(c.name.clone()),
                })
                .ok_or_else(|| "Contact not found".into()),
            CallTarget::PeerId(id) => {
                Ok(ResolvedTarget { peer_id: Some(id.clone()), address: None, name: None })
            }
            CallTarget::Address(addr) => {
                Ok(ResolvedTarget { peer_id: None, address: Some(addr.clone()), name: None })
            }
        }
    }
}

pub struct ResolvedTarget {
    peer_id: Option<String>,
    address: Option<String>,
    name: Option<String>,
}

impl Drop for CallService {
    fn drop(&mut self) {
        if let Some(task) = self.event_task.take() {
            tracing::debug!("Aborting CallService task...");
            task.abort();
        }
    }
}

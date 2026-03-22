use std::net::SocketAddr;

use super::{CallService, CallState};
use crate::{
    networking::discovery::PeerInfo, services::contacts_service::Contact, ui::message::CallTarget,
};

/// Result of a successful dial operation.
pub struct DialSuccess {
    pub peer_id: Option<String>,
    pub name: Option<String>,
    pub socket_addr: Option<SocketAddr>,
    pub update_contact_address: Option<(i64, String)>,
}

pub type DialResult = Result<DialSuccess, String>;

pub(super) struct ResolvedTarget {
    peer_id: Option<String>,
    address: Option<String>,
    name: Option<String>,
}

impl CallService {
    pub async fn dial(
        &self,
        target: CallTarget,
        contacts: &[Contact],
        discovered: &[PeerInfo],
    ) -> DialResult {
        *self.state.write().unwrap() = CallState::Dialing { target: target.clone() };

        let ResolvedTarget { peer_id: tid, address: taddr, name: tname } =
            match self.resolve_target(&target, contacts) {
                Ok(target) => target,
                Err(err) => {
                    *self.state.write().unwrap() = CallState::Idle;
                    return Err(err);
                }
            };

        *self.webrtc.remote_peer_id.write().await = tid.clone();

        if let Some(id) = &tid
            && let Some(peer) = discovered.iter().find(|peer| peer.id == *id)
        {
            for addr in &peer.addresses {
                let socket_addr = SocketAddr::new(*addr, peer.port);
                if self.webrtc.dial_direct(socket_addr).await.is_ok() {
                    let mut update_contact_address = None;
                    if let CallTarget::ContactId(contact_id) = target {
                        let new_address = socket_addr.to_string();
                        if taddr.as_ref() != Some(&new_address) {
                            update_contact_address = Some((contact_id, new_address));
                        }
                    }

                    if let Err(err) = self.webrtc.create_offer().await {
                        self.rollback_failed_dial().await;
                        return Err(format!("Offer failed: {}", err));
                    }

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

        if let Some(addr_str) = taddr {
            let addr = match addr_str.parse::<SocketAddr>() {
                Ok(addr) => addr,
                Err(_) => {
                    *self.state.write().unwrap() = CallState::Idle;
                    *self.webrtc.remote_peer_id.write().await = None;
                    return Err("Invalid address format".into());
                }
            };

            if self.webrtc.dial_direct(addr).await.is_ok() {
                if let Err(err) = self.webrtc.create_offer().await {
                    self.rollback_failed_dial().await;
                    return Err(format!("Offer failed: {}", err));
                }

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
        *self.webrtc.remote_peer_id.write().await = None;
        Err("Connection failed".into())
    }

    pub(super) async fn rollback_failed_dial(&self) {
        self.webrtc.reset_after_failed_dial().await;
        *self.state.write().unwrap() = CallState::Idle;
    }

    fn resolve_target(
        &self,
        target: &CallTarget,
        contacts: &[Contact],
    ) -> Result<ResolvedTarget, String> {
        match target {
            CallTarget::ContactId(id) => contacts
                .iter()
                .find(|contact| contact.id == *id)
                .map(|contact| ResolvedTarget {
                    peer_id: Some(contact.peer_id.clone()),
                    address: contact.address.clone(),
                    name: Some(contact.name.clone()),
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

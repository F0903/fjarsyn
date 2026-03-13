use std::net::SocketAddr;

use crate::{
    database::Contact,
    networking::{discovery::PeerInfo, webrtc::WebRTC},
    ui::message::CallTarget,
};

pub enum DialResult {
    Success {
        peer_id: Option<String>,
        name: Option<String>,
        socket_addr: Option<SocketAddr>,
        update_contact_address: Option<(i64, String)>,
    },
    Failure(String),
}

#[derive(Clone, Debug)]
pub struct CallService {
    webrtc: WebRTC,
}

impl CallService {
    pub fn new(webrtc: WebRTC) -> Self {
        Self { webrtc }
    }

    pub fn webrtc(&self) -> &WebRTC {
        &self.webrtc
    }

    pub async fn accept_call(&self) -> Result<(), crate::networking::webrtc::WebRTCError> {
        self.webrtc.accept_call().await
    }

    pub async fn decline_call(&self) -> Result<(), crate::networking::webrtc::WebRTCError> {
        self.webrtc.decline_call().await
    }

    pub async fn disconnect(&self) -> Result<(), crate::networking::webrtc::WebRTCError> {
        self.webrtc.disconnect().await
    }

    pub fn resolve_target(
        &self,
        target: &CallTarget,
        contacts: &[Contact],
    ) -> Result<(Option<String>, Option<String>, Option<String>), String> {
        match target {
            CallTarget::ContactId(id) => contacts
                .iter()
                .find(|c| c.id == *id)
                .map(|c| (Some(c.peer_id.clone()), c.address.clone(), Some(c.name.clone())))
                .ok_or_else(|| "Contact not found".into()),
            CallTarget::PeerId(id) => Ok((Some(id.clone()), None, None)),
            CallTarget::Address(addr) => Ok((None, Some(addr.clone()), None)),
        }
    }

    pub async fn dial(
        &self,
        target: CallTarget,
        contacts: &[Contact],
        discovered: &[PeerInfo],
    ) -> DialResult {
        let (tid, taddr, tname) = match self.resolve_target(&target, contacts) {
            Ok(res) => res,
            Err(e) => return DialResult::Failure(e),
        };

        if let Some(id) = &tid {
            if let Some(p) = discovered.iter().find(|p| p.id == *id) {
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

                        if let Err(e) = self.webrtc.create_offer().await {
                            return DialResult::Failure(format!("Offer failed: {}", e));
                        }

                        return DialResult::Success {
                            peer_id: tid,
                            name: tname,
                            socket_addr: None,
                            update_contact_address,
                        };
                    }
                }
            }
        }

        if let Some(addr_str) = taddr {
            if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                if self.webrtc.dial_direct(addr).await.is_ok() {
                    if let Err(e) = self.webrtc.create_offer().await {
                        return DialResult::Failure(format!("Offer failed: {}", e));
                    }

                    return DialResult::Success {
                        peer_id: tid,
                        name: tname,
                        socket_addr: Some(addr),
                        update_contact_address: None,
                    };
                }
            } else {
                return DialResult::Failure("Invalid address format".into());
            }
        }

        DialResult::Failure("Connection failed".into())
    }
}

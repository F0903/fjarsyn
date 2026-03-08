use std::{collections::HashMap, net::IpAddr};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: String,
    pub instance_name: String,
    pub host_name: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
}

impl PeerInfo {
    /// Merges addresses from another PeerInfo into this one, ensuring no duplicates.
    /// Also updates the instance name and port.
    pub fn update(&mut self, other: PeerInfo) {
        for addr in other.addresses {
            if !self.addresses.contains(&addr) {
                self.addresses.push(addr);
            }
        }
        self.instance_name = other.instance_name;
        self.port = other.port;

        // Re-sort to maintain prioritization (Tailscale > Localhost > LAN)
        self.addresses.sort_by_key(|addr| {
            if let IpAddr::V4(v4) = addr {
                match v4.octets()[0] {
                    100 => 0,
                    127 => 1,
                    10 | 172 | 192 => 2,
                    _ => 3,
                }
            } else {
                4
            }
        });
    }
}

#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    PeerFound(PeerInfo),
    PeerRemoved(String), // By Fullname
}

pub struct Discovery {
    daemon: ServiceDaemon,
    service_type: String,
}

impl Discovery {
    pub const SERVICE_TYPE: &'static str = "_fjarsyn._tcp.local.";

    pub fn new() -> Result<Self, mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;
        Ok(Self { daemon, service_type: Self::SERVICE_TYPE.to_string() })
    }

    pub fn advertise(&self, peer_id: &str, port: u16) -> Result<(), mdns_sd::Error> {
        let instance_name = format!("fjarsyn-{}", peer_id);
        let host_name = format!("{}.local.", instance_name);

        let mut properties = HashMap::new();
        properties.insert("peer_id".to_string(), peer_id.to_string());

        let service_info = ServiceInfo::new(
            &self.service_type,
            &instance_name,
            &host_name,
            "", // Auto-detect IP
            port,
            Some(properties),
        )?
        .enable_addr_auto();

        tracing::info!("Advertising mDNS service: {} on port {}", instance_name, port);
        self.daemon.register(service_info)
    }

    pub fn browse(&self, tx: mpsc::Sender<DiscoveryEvent>) -> Result<(), mdns_sd::Error> {
        let receiver = self.daemon.browse(&self.service_type)?;

        tokio::spawn(async move {
            while let Ok(event) = receiver.recv_async().await {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        if let Some(id) = info.get_property_val_str("peer_id") {
                            let mut ipv4_addresses: Vec<IpAddr> = info
                                .get_addresses()
                                .iter()
                                .filter(|addr| addr.is_ipv4())
                                .cloned()
                                .collect();

                            if ipv4_addresses.is_empty() {
                                continue;
                            }

                            ipv4_addresses.sort_by_key(|addr| {
                                if let IpAddr::V4(v4) = addr {
                                    match v4.octets()[0] {
                                        100 => 0,
                                        127 => 1,
                                        10 | 172 | 192 => 2,
                                        _ => 3,
                                    }
                                } else {
                                    4
                                }
                            });

                            let peer = PeerInfo {
                                id: id.to_string(),
                                instance_name: info.get_fullname().to_string(),
                                host_name: info.get_hostname().to_string(),
                                addresses: ipv4_addresses,
                                port: info.get_port(),
                            };
                            let _ = tx.send(DiscoveryEvent::PeerFound(peer)).await;
                        }
                    }
                    ServiceEvent::ServiceRemoved(_type, fullname) => {
                        let _ = tx.send(DiscoveryEvent::PeerRemoved(fullname)).await;
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }
}

use std::{collections::HashMap, net::IpAddr};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;

pub mod model;

use model::address_priority;
pub use model::{DiscoveryEvent, PeerInfo};

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

                            ipv4_addresses.sort_by_key(address_priority);

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

    pub fn shutdown(&self) -> Result<(), mdns_sd::Error> {
        self.daemon.shutdown().map(|_| ())
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        if let Err(err) = self.shutdown() {
            tracing::debug!("Failed to shut down mDNS discovery daemon cleanly: {}", err);
        }
    }
}

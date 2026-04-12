use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: String,
    pub instance_name: String,
    pub host_name: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
}

impl PeerInfo {
    pub fn update(&mut self, other: PeerInfo) {
        for addr in other.addresses {
            if !self.addresses.contains(&addr) {
                self.addresses.push(addr);
            }
        }
        self.instance_name = other.instance_name;
        self.port = other.port;
        self.addresses.sort_by_key(address_priority);
    }
}

#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    PeerFound(PeerInfo),
    PeerRemoved(String),
}

pub fn address_priority(addr: &IpAddr) -> u8 {
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
}

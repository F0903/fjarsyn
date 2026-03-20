use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NetworkConfig {
    pub max_depacket_latency: u16,
}

impl NetworkConfig {
    pub const DEFAULT_MAX_DEPACKET_LATENCY_MS: u16 = 50;
    pub const MAX_DEPACKET_LATENCY_MS: u16 = 1000;

    pub fn normalize(&mut self) {
        self.max_depacket_latency =
            self.max_depacket_latency.clamp(0, Self::MAX_DEPACKET_LATENCY_MS);
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self { max_depacket_latency: Self::DEFAULT_MAX_DEPACKET_LATENCY_MS }
    }
}

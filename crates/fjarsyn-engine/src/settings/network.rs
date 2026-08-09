use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Network {
    pub max_depacket_latency_ms: u16,
}

impl Network {
    pub const DEFAULT_MAX_DEPACKET_LATENCY_MS: u16 = 50;
    pub const MAX_DEPACKET_LATENCY_MS: u16 = 1000;

    pub(crate) fn normalize(&mut self) {
        self.max_depacket_latency_ms =
            self.max_depacket_latency_ms.clamp(0, Self::MAX_DEPACKET_LATENCY_MS);
    }
}

impl Default for Network {
    fn default() -> Self {
        Self { max_depacket_latency_ms: Self::DEFAULT_MAX_DEPACKET_LATENCY_MS }
    }
}

use std::net::IpAddr;

use webrtc::{
    api::setting_engine::SettingEngine,
    ice::{mdns::MulticastDnsMode, network_type::NetworkType},
};

use crate::peer_session::{Error, NetworkScope};

/// Applies the application network scope to WebRTC socket creation and to
/// peer-supplied ICE endpoints before they reach the ICE agent.
#[derive(Debug, Clone, Copy)]
pub(super) struct NetworkPolicy {
    scope: NetworkScope,
}

impl NetworkPolicy {
    pub(super) fn new(scope: NetworkScope, ice_servers: &[String]) -> Result<Self, Error> {
        if scope == NetworkScope::LoopbackOnly && !ice_servers.is_empty() {
            return Err(Error::Protocol(
                "loopback-only WebRTC cannot use STUN or TURN servers".into(),
            ));
        }
        Ok(Self { scope })
    }

    pub(super) fn setting_engine(self) -> SettingEngine {
        let mut settings = SettingEngine::default();
        if self.scope == NetworkScope::LoopbackOnly {
            settings.set_network_types(vec![NetworkType::Udp4, NetworkType::Udp6]);
            settings.set_include_loopback_candidate(true);
            settings.set_ip_filter(Box::new(|address| address.is_loopback()));
            // QueryOnly is WebRTC's default and still opens an all-interface
            // multicast socket on Windows. Numeric loopback candidates need no
            // multicast name discovery.
            settings.set_ice_multicast_dns_mode(MulticastDnsMode::Disabled);
        }
        settings
    }

    pub(super) fn validate_candidate(self, candidate: &str) -> Result<(), Error> {
        if self.scope == NetworkScope::AllInterfaces || candidate.is_empty() {
            return Ok(());
        }

        let address = candidate_address(candidate).ok_or_else(|| {
            Error::Protocol(
                "loopback-only WebRTC received an ICE candidate without a numeric address".into(),
            )
        })?;
        if is_loopback(address) {
            Ok(())
        } else {
            Err(Error::Protocol(
                "loopback-only WebRTC rejected a non-loopback ICE candidate".into(),
            ))
        }
    }

    pub(super) fn validate_remote_sdp(self, sdp: &str) -> Result<(), Error> {
        if self.scope == NetworkScope::AllInterfaces {
            return Ok(());
        }

        for line in sdp.lines().map(|line| line.trim_end_matches('\r')) {
            if let Some(candidate) = line.strip_prefix("a=candidate:") {
                self.validate_candidate(&format!("candidate:{candidate}"))?;
            }
        }
        Ok(())
    }
}

fn candidate_address(candidate: &str) -> Option<IpAddr> {
    candidate.strip_prefix("candidate:")?.split_ascii_whitespace().nth(4)?.parse().ok()
}

fn is_loopback(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.is_loopback(),
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.to_ipv4_mapped().is_some_and(|address| address.is_loopback())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loopback_policy() -> NetworkPolicy {
        NetworkPolicy::new(NetworkScope::LoopbackOnly, &[]).unwrap()
    }

    #[test]
    fn loopback_policy_rejects_external_ice_servers() {
        assert!(matches!(
            NetworkPolicy::new(NetworkScope::LoopbackOnly, &["stun:stun.example.test".into()]),
            Err(Error::Protocol(_))
        ));
        assert!(
            NetworkPolicy::new(NetworkScope::AllInterfaces, &["stun:stun.example.test".into()])
                .is_ok()
        );
    }

    #[test]
    fn loopback_policy_accepts_only_numeric_loopback_candidates() {
        let policy = loopback_policy();

        for candidate in [
            "candidate:1 1 udp 1 127.0.0.1 9000 typ host",
            "candidate:2 1 udp 1 ::1 9001 typ host",
            "candidate:3 1 udp 1 ::ffff:127.0.0.1 9002 typ host",
        ] {
            policy.validate_candidate(candidate).unwrap();
        }
        for candidate in [
            "candidate:4 1 udp 1 192.168.1.10 9000 typ host",
            "candidate:5 1 udp 1 fe80::1 9000 typ host",
            "candidate:6 1 udp 1 peer.local 9000 typ host",
            "malformed",
        ] {
            assert!(matches!(policy.validate_candidate(candidate), Err(Error::Protocol(_))));
        }
    }

    #[test]
    fn loopback_policy_checks_candidates_embedded_in_remote_sdp() {
        let policy = loopback_policy();
        policy
            .validate_remote_sdp("v=0\r\na=candidate:1 1 udp 1 127.0.0.1 9000 typ host\r\n")
            .unwrap();
        assert!(matches!(
            policy.validate_remote_sdp("v=0\r\na=candidate:1 1 udp 1 10.0.0.1 9000 typ host\r\n"),
            Err(Error::Protocol(_))
        ));
    }
}

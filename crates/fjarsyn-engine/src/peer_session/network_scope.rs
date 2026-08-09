use std::net::IpAddr;

/// Controls which network interfaces a peer session may expose or contact.
///
/// Production sessions use [`AllInterfaces`](Self::AllInterfaces) so nearby
/// peers can establish signaling and WebRTC transports across the LAN. Tests
/// can select [`LoopbackOnly`](Self::LoopbackOnly) to guarantee that their
/// network traffic never leaves the local machine.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkScope {
    /// Listen and connect on every otherwise-valid network interface.
    #[default]
    AllInterfaces,
    /// Listen and connect only through IPv4 or IPv6 loopback.
    LoopbackOnly,
}

impl NetworkScope {
    pub(crate) const fn allows(self, address: IpAddr) -> bool {
        match self {
            Self::AllInterfaces => true,
            Self::LoopbackOnly => address.is_loopback(),
        }
    }
}

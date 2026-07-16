use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Instant,
};

/// Hard application-level bounds for unauthenticated mDNS presence state.
///
/// Presence admission is deliberately stable under pressure: an observation
/// for an already-admitted instance may refresh or replace that instance, but
/// a new peer or instance that would exceed a limit is ignored. Existing state
/// is never evicted merely because an unauthenticated newcomer arrived, and a
/// later removal frees capacity for a subsequent observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresenceLimits {
    /// Maximum distinct peer IDs retained at once. Defaults to 256.
    pub max_peers: usize,
    /// Maximum current mDNS instances retained for one claimed peer ID.
    /// Defaults to 4.
    pub max_advertisements_per_peer: usize,
    /// Maximum de-duplicated endpoints retained from one advertisement.
    /// Defaults to 16.
    pub max_endpoints_per_advertisement: usize,
    /// Maximum de-duplicated endpoints exposed for one aggregate nearby peer.
    /// Defaults to 32.
    pub max_endpoints_per_peer: usize,
}

impl Default for PresenceLimits {
    fn default() -> Self {
        Self {
            max_peers: 256,
            max_advertisements_per_peer: 4,
            max_endpoints_per_advertisement: 16,
            max_endpoints_per_peer: 32,
        }
    }
}

impl PresenceLimits {
    pub(crate) fn zero_limit(self) -> Option<&'static str> {
        [
            ("max_peers", self.max_peers),
            ("max_advertisements_per_peer", self.max_advertisements_per_peer),
            ("max_endpoints_per_advertisement", self.max_endpoints_per_advertisement),
            ("max_endpoints_per_peer", self.max_endpoints_per_peer),
        ]
        .into_iter()
        .find_map(|(name, value)| (value == 0).then_some(name))
    }
}

/// One mDNS advertisement contributing reachability hints for a nearby peer.
///
/// An advertisement is not proof of identity. Its endpoints must only be used to
/// bootstrap an authenticated connection to an already trusted peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearbyAdvertisement {
    pub instance_name: String,
    pub hostname: String,
    pub endpoints: Arc<[SocketAddr]>,
    pub last_seen: Instant,
}

/// The aggregate presence of one peer across all of its current mDNS
/// advertisements.
///
/// `hostname` and `instance_name` identify the most recently resolved
/// advertisement. `endpoints` is the de-duplicated union of every current
/// advertisement for this peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearbyPeer {
    pub peer_id: String,
    pub hostname: String,
    pub instance_name: String,
    pub endpoints: Arc<[SocketAddr]>,
    pub last_seen: Instant,
    pub advertisements: Arc<[NearbyAdvertisement]>,
}

impl NearbyPeer {
    /// Returns all current reachability hints for this peer.
    ///
    /// These addresses are unauthenticated mDNS data. Successful connection
    /// authentication, never endpoint selection, establishes the remote peer's
    /// identity.
    pub fn endpoint_hints(&self) -> Arc<[SocketAddr]> {
        self.endpoints.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceSnapshot {
    revision: u64,
    peers: Arc<[NearbyPeer]>,
}

impl Default for PresenceSnapshot {
    fn default() -> Self {
        Self { revision: 0, peers: Arc::from([]) }
    }
}

impl PresenceSnapshot {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn peers(&self) -> &[NearbyPeer] {
        &self.peers
    }

    pub fn peer(&self, peer_id: &str) -> Option<&NearbyPeer> {
        self.peers.iter().find(|peer| peer.peer_id == peer_id)
    }

    pub fn is_nearby(&self, peer_id: &str) -> bool {
        self.peer(peer_id).is_some()
    }

    /// Returns all current endpoint hints for a peer, or an empty slice when
    /// the peer is not nearby.
    ///
    /// Presence is deliberately unauthenticated. Callers must bind the
    /// resulting connection to the peer's trusted identity and must never
    /// treat selection of one of these addresses as identity proof.
    pub fn endpoint_hints(&self, peer_id: &str) -> Arc<[SocketAddr]> {
        self.peer(peer_id).map(NearbyPeer::endpoint_hints).unwrap_or_else(|| Arc::from([]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAdvertisement {
    pub peer_id: String,
    pub advertisement: NearbyAdvertisement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PresenceObservation {
    Resolved(ResolvedAdvertisement),
    Removed { instance_name: String },
}

#[derive(Debug)]
pub(crate) struct PresenceRegistry {
    by_instance: HashMap<String, ResolvedAdvertisement>,
    revision: u64,
    limits: PresenceLimits,
}

impl Default for PresenceRegistry {
    fn default() -> Self {
        Self::new(PresenceLimits::default())
    }
}

impl PresenceRegistry {
    pub fn new(limits: PresenceLimits) -> Self {
        debug_assert!(limits.zero_limit().is_none());
        Self { by_instance: HashMap::new(), revision: 0, limits }
    }

    pub fn apply(&mut self, observation: PresenceObservation) -> bool {
        let changed = match observation {
            PresenceObservation::Resolved(resolved) => self.upsert(resolved),
            PresenceObservation::Removed { instance_name } => {
                self.by_instance.remove(&instance_key(&instance_name)).is_some()
            }
        };

        if changed {
            self.revision = self.revision.wrapping_add(1);
        }

        changed
    }

    pub fn clear(&mut self) -> bool {
        if self.by_instance.is_empty() {
            return false;
        }

        self.by_instance.clear();
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn snapshot(&self) -> PresenceSnapshot {
        let mut grouped = HashMap::<String, Vec<NearbyAdvertisement>>::new();
        for resolved in self.by_instance.values() {
            grouped
                .entry(resolved.peer_id.clone())
                .or_default()
                .push(resolved.advertisement.clone());
        }

        let mut peers = grouped
            .into_iter()
            .filter_map(|(peer_id, mut advertisements)| {
                advertisements.sort_by(|left, right| {
                    right
                        .last_seen
                        .cmp(&left.last_seen)
                        .then_with(|| left.instance_name.cmp(&right.instance_name))
                });

                let primary = advertisements.first()?.clone();
                let mut endpoints = advertisements
                    .iter()
                    .flat_map(|advertisement| advertisement.endpoints.iter().copied())
                    .collect::<Vec<_>>();
                normalize_endpoint_hints(&mut endpoints, self.limits.max_endpoints_per_peer);

                Some(NearbyPeer {
                    peer_id,
                    hostname: primary.hostname,
                    instance_name: primary.instance_name,
                    endpoints: endpoints.into(),
                    last_seen: primary.last_seen,
                    advertisements: advertisements.into(),
                })
            })
            .collect::<Vec<_>>();
        peers.sort_by(|left, right| left.peer_id.cmp(&right.peer_id));

        PresenceSnapshot { revision: self.revision, peers: peers.into() }
    }

    fn upsert(&mut self, mut resolved: ResolvedAdvertisement) -> bool {
        if resolved.peer_id.trim().is_empty()
            || resolved.advertisement.instance_name.trim().is_empty()
            || resolved.advertisement.endpoints.is_empty()
        {
            return false;
        }

        let mut endpoints = resolved.advertisement.endpoints.to_vec();
        normalize_endpoint_hints(&mut endpoints, self.limits.max_endpoints_per_advertisement);
        if endpoints.is_empty() {
            return false;
        }
        resolved.advertisement.endpoints = endpoints.into();

        let key = instance_key(&resolved.advertisement.instance_name);
        if self.by_instance.get(&key) == Some(&resolved) {
            return false;
        }

        if !self.can_admit(&key, &resolved.peer_id) {
            return false;
        }

        self.by_instance.insert(key, resolved);
        true
    }

    fn can_admit(&self, replacing_key: &str, target_peer_id: &str) -> bool {
        let mut peers_without_replaced_instance = HashSet::new();
        let mut target_advertisements = 0;

        for (key, resolved) in &self.by_instance {
            if key == replacing_key {
                continue;
            }

            peers_without_replaced_instance.insert(resolved.peer_id.as_str());
            if resolved.peer_id == target_peer_id {
                target_advertisements += 1;
            }
        }

        if target_advertisements >= self.limits.max_advertisements_per_peer {
            return false;
        }

        peers_without_replaced_instance.contains(target_peer_id)
            || peers_without_replaced_instance.len() < self.limits.max_peers
    }
}

fn instance_key(instance_name: &str) -> String {
    // DNS names are case-insensitive. Normalizing also ensures a removal using
    // different casing still removes only the intended advertisement.
    instance_name.to_ascii_lowercase()
}

/// Filters, deterministically orders, de-duplicates and caps endpoint hints.
///
/// When both IP families are available and `limit` permits at least two
/// entries, the final retained slot is reserved for the otherwise absent
/// family. This prevents the IPv4-first preference order from erasing every
/// IPv6 fallback while retaining stable ordering within each family.
pub(crate) fn normalize_endpoint_hints(endpoints: &mut Vec<SocketAddr>, limit: usize) {
    for endpoint in endpoints.iter_mut() {
        if let SocketAddr::V6(address) = *endpoint
            && let Some(mapped) = address.ip().to_ipv4_mapped()
        {
            *endpoint = SocketAddr::new(IpAddr::V4(mapped), address.port());
        }
    }
    endpoints.retain(endpoint_is_usable);
    let mut seen = HashSet::with_capacity(endpoints.len());
    endpoints.retain(|endpoint| seen.insert(*endpoint));
    endpoints.sort_by_key(|endpoint| (address_priority(&endpoint.ip()), *endpoint));

    if endpoints.len() <= limit {
        return;
    }
    if limit == 0 {
        endpoints.clear();
        return;
    }

    let first_ipv4 = endpoints.iter().find(|endpoint| endpoint.is_ipv4()).copied();
    let first_ipv6 = endpoints.iter().find(|endpoint| endpoint.is_ipv6()).copied();
    endpoints.truncate(limit);

    if limit < 2 || first_ipv4.is_none() || first_ipv6.is_none() {
        return;
    }

    let retained_ipv4 = endpoints.iter().any(SocketAddr::is_ipv4);
    let retained_ipv6 = endpoints.iter().any(SocketAddr::is_ipv6);
    let missing_family_endpoint = match (retained_ipv4, retained_ipv6) {
        (false, true) => first_ipv4,
        (true, false) => first_ipv6,
        _ => None,
    };
    if let Some(endpoint) = missing_family_endpoint {
        endpoints[limit - 1] = endpoint;
    }
}

fn endpoint_is_usable(endpoint: &SocketAddr) -> bool {
    if endpoint.port() == 0 {
        return false;
    }

    match endpoint.ip() {
        IpAddr::V4(address) => {
            !address.is_unspecified()
                && !address.is_multicast()
                && address != std::net::Ipv4Addr::BROADCAST
        }
        IpAddr::V6(address) => !address.is_unspecified() && !address.is_multicast(),
    }
}

fn address_priority(address: &IpAddr) -> u8 {
    match address {
        IpAddr::V4(address) => match address.octets()[0] {
            // Prefer overlay and loopback routes before private LAN routes to
            // retain the project's current endpoint-selection behavior.
            100 => 0,
            127 => 1,
            10 | 172 | 192 => 2,
            _ => 3,
        },
        IpAddr::V6(_) => 4,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, Ipv6Addr, SocketAddrV6},
        time::Duration,
    };

    use super::*;

    fn resolved(
        peer_id: &str,
        instance_name: &str,
        hostname: &str,
        endpoint: SocketAddr,
        last_seen: Instant,
    ) -> PresenceObservation {
        resolved_with_endpoints(peer_id, instance_name, hostname, vec![endpoint], last_seen)
    }

    fn resolved_with_endpoints(
        peer_id: &str,
        instance_name: &str,
        hostname: &str,
        endpoints: Vec<SocketAddr>,
        last_seen: Instant,
    ) -> PresenceObservation {
        PresenceObservation::Resolved(ResolvedAdvertisement {
            peer_id: peer_id.into(),
            advertisement: NearbyAdvertisement {
                instance_name: instance_name.into(),
                hostname: hostname.into(),
                endpoints: endpoints.into(),
                last_seen,
            },
        })
    }

    fn limits(
        max_peers: usize,
        max_advertisements_per_peer: usize,
        max_endpoints_per_advertisement: usize,
        max_endpoints_per_peer: usize,
    ) -> PresenceLimits {
        PresenceLimits {
            max_peers,
            max_advertisements_per_peer,
            max_endpoints_per_advertisement,
            max_endpoints_per_peer,
        }
    }

    #[test]
    fn aggregates_multiple_advertisements_for_one_peer() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::default();
        let first = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 4), 9000));
        let second = SocketAddr::from((Ipv4Addr::new(100, 64, 0, 4), 9001));

        assert!(registry.apply(resolved(
            "peer-a",
            "a-one._fjarsyn._tcp.local.",
            "a-one.local.",
            first,
            now,
        )));
        assert!(registry.apply(resolved(
            "peer-a",
            "a-two._fjarsyn._tcp.local.",
            "a-two.local.",
            second,
            now + Duration::from_secs(1),
        )));

        let snapshot = registry.snapshot();
        let peer = snapshot.peer("peer-a").unwrap();
        assert_eq!(snapshot.peers().len(), 1);
        assert_eq!(peer.advertisements.len(), 2);
        assert_eq!(peer.instance_name, "a-two._fjarsyn._tcp.local.");
        assert_eq!(peer.endpoints.as_ref(), &[second, first]);
        assert_eq!(snapshot.endpoint_hints("peer-a").as_ref(), &[second, first]);
        assert!(snapshot.endpoint_hints("unknown-peer").is_empty());
    }

    #[test]
    fn removal_only_removes_the_matching_advertisement() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::default();
        let first = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 4), 9000));
        let second = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 5), 9001));

        registry.apply(resolved(
            "peer-a",
            "a-one._fjarsyn._tcp.local.",
            "a-one.local.",
            first,
            now,
        ));
        registry.apply(resolved(
            "peer-a",
            "a-two._fjarsyn._tcp.local.",
            "a-two.local.",
            second,
            now + Duration::from_secs(1),
        ));

        assert!(registry.apply(PresenceObservation::Removed {
            instance_name: "A-TWO._FJARSYN._TCP.LOCAL.".into(),
        }));

        let peer = registry.snapshot().peer("peer-a").unwrap().clone();
        assert_eq!(peer.advertisements.len(), 1);
        assert_eq!(peer.instance_name, "a-one._fjarsyn._tcp.local.");
        assert_eq!(peer.endpoints.as_ref(), &[first]);
    }

    #[test]
    fn resolving_an_existing_instance_replaces_only_that_instance() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::default();
        let old_endpoint = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 4), 9000));
        let new_endpoint = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 4), 9000));

        registry.apply(resolved(
            "peer-a",
            "instance._fjarsyn._tcp.local.",
            "old.local.",
            old_endpoint,
            now,
        ));
        registry.apply(resolved(
            "peer-a",
            "INSTANCE._fjarsyn._tcp.local.",
            "new.local.",
            new_endpoint,
            now + Duration::from_secs(1),
        ));

        let peer = registry.snapshot().peer("peer-a").unwrap().clone();
        assert_eq!(peer.advertisements.len(), 1);
        assert_eq!(peer.hostname, "new.local.");
        assert_eq!(peer.endpoints.as_ref(), &[new_endpoint]);
    }

    #[test]
    fn endpoint_limits_are_deduplicated_sorted_and_applied_at_both_levels() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::new(limits(2, 2, 2, 3));
        let private_one = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 1), 9000));
        let private_two = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 2), 9000));
        let private_three = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 3), 9000));
        let lan_one = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 9000));
        let lan_two = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 2), 9000));
        let lan_three = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 3), 9000));

        assert!(registry.apply(resolved_with_endpoints(
            "peer-a",
            "a-one._fjarsyn._tcp.local.",
            "a-one.local.",
            vec![private_three, private_one, private_two, private_one],
            now,
        )));
        assert!(registry.apply(resolved_with_endpoints(
            "peer-a",
            "a-two._fjarsyn._tcp.local.",
            "a-two.local.",
            vec![lan_three, lan_two, lan_one],
            now + Duration::from_secs(1),
        )));

        let snapshot = registry.snapshot();
        let peer = snapshot.peer("peer-a").unwrap();
        assert_eq!(peer.advertisements.len(), 2);
        assert_eq!(peer.advertisements[0].endpoints.as_ref(), &[lan_one, lan_two]);
        assert_eq!(peer.advertisements[1].endpoints.as_ref(), &[private_one, private_two]);
        assert_eq!(peer.endpoints.as_ref(), &[lan_one, lan_two, private_one]);
    }

    #[test]
    fn endpoint_normalization_filters_unusable_hints_and_preserves_family_diversity() {
        let ipv4_one = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 9000));
        let ipv4_two = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 2), 9000));
        let scoped_ipv6 = SocketAddr::V6(SocketAddrV6::new(
            "fe80::1234".parse::<Ipv6Addr>().unwrap(),
            9000,
            0,
            17,
        ));
        let mut endpoints = vec![
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 9000)),
            SocketAddr::from((Ipv4Addr::new(224, 0, 0, 1), 9000)),
            SocketAddr::from((Ipv4Addr::BROADCAST, 9000)),
            SocketAddr::from((Ipv4Addr::new(10, 0, 0, 9), 0)),
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 9000, 0, 0)),
            SocketAddr::V6(SocketAddrV6::new("ff02::1".parse::<Ipv6Addr>().unwrap(), 9000, 0, 17)),
            ipv4_two,
            SocketAddr::V6(SocketAddrV6::new(
                Ipv4Addr::new(10, 0, 0, 1).to_ipv6_mapped(),
                ipv4_one.port(),
                0,
                0,
            )),
            scoped_ipv6,
            ipv4_one,
            scoped_ipv6,
        ];

        normalize_endpoint_hints(&mut endpoints, 2);

        assert_eq!(endpoints, [ipv4_one, scoped_ipv6]);
        assert_eq!(
            endpoints[1],
            SocketAddr::V6(SocketAddrV6::new(
                "fe80::1234".parse::<Ipv6Addr>().unwrap(),
                9000,
                0,
                17,
            ))
        );
    }

    #[test]
    fn per_advertisement_and_aggregate_caps_each_preserve_both_ip_families() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::new(limits(2, 2, 2, 2));
        let ipv4_one = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 9000));
        let ipv4_two = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 2), 9000));
        let ipv4_three = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 3), 9000));
        let scoped_ipv6 = SocketAddr::V6(SocketAddrV6::new(
            "fe80::5678".parse::<Ipv6Addr>().unwrap(),
            9000,
            0,
            23,
        ));

        assert!(registry.apply(resolved_with_endpoints(
            "peer-a",
            "a-one._fjarsyn._tcp.local.",
            "a-one.local.",
            vec![ipv4_three, scoped_ipv6, ipv4_two, ipv4_one],
            now,
        )));
        let single = registry.snapshot();
        let single_peer = single.peer("peer-a").unwrap();
        assert_eq!(single_peer.advertisements[0].endpoints.as_ref(), &[ipv4_one, scoped_ipv6]);
        assert_eq!(single_peer.endpoints.as_ref(), &[ipv4_one, scoped_ipv6]);

        assert!(registry.apply(resolved_with_endpoints(
            "peer-a",
            "a-two._fjarsyn._tcp.local.",
            "a-two.local.",
            vec![ipv4_two, ipv4_three],
            now + Duration::from_secs(1),
        )));
        let aggregate = registry.snapshot();
        let aggregate_peer = aggregate.peer("peer-a").unwrap();
        assert_eq!(aggregate_peer.endpoints.as_ref(), &[ipv4_one, scoped_ipv6]);
        assert!(matches!(
            aggregate_peer.endpoints[1],
            SocketAddr::V6(address) if address.scope_id() == 23
        ));
    }

    #[test]
    fn unusable_only_advertisements_are_rejected_without_consuming_capacity() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::new(limits(1, 1, 1, 1));
        let unusable = vec![
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 9000)),
            SocketAddr::from((Ipv4Addr::BROADCAST, 9000)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        ];

        assert!(!registry.apply(resolved_with_endpoints(
            "attacker",
            "bad._fjarsyn._tcp.local.",
            "bad.local.",
            unusable,
            now,
        )));
        assert!(registry.snapshot().peers().is_empty());

        let usable = SocketAddr::from((Ipv4Addr::LOCALHOST, 9000));
        assert!(registry.apply(resolved(
            "peer-a",
            "good._fjarsyn._tcp.local.",
            "good.local.",
            usable,
            now,
        )));
        assert_eq!(registry.snapshot().peer("peer-a").unwrap().endpoints.as_ref(), &[usable]);
    }

    #[test]
    fn full_peer_registry_rejects_newcomers_but_refresh_and_removal_free_capacity() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::new(limits(2, 2, 2, 4));
        let endpoint_a = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 9000));
        let endpoint_a_refreshed = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 2), 9000));
        let endpoint_b = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 3), 9000));
        let endpoint_c = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 4), 9000));
        let instance_a = "a._fjarsyn._tcp.local.";
        let instance_b = "b._fjarsyn._tcp.local.";
        let instance_c = "c._fjarsyn._tcp.local.";

        assert!(registry.apply(resolved("peer-a", instance_a, "a.local.", endpoint_a, now)));
        assert!(registry.apply(resolved("peer-b", instance_b, "b.local.", endpoint_b, now)));
        let full_revision = registry.snapshot().revision();

        assert!(!registry.apply(resolved("peer-c", instance_c, "c.local.", endpoint_c, now)));
        assert_eq!(registry.snapshot().revision(), full_revision);
        assert!(registry.snapshot().peer("peer-c").is_none());

        assert!(registry.apply(resolved(
            "peer-a",
            "A._FJARSYN._TCP.LOCAL.",
            "a-new.local.",
            endpoint_a_refreshed,
            now + Duration::from_secs(1),
        )));
        assert_eq!(
            registry.snapshot().peer("peer-a").unwrap().endpoints.as_ref(),
            &[endpoint_a_refreshed]
        );

        assert!(registry.apply(PresenceObservation::Removed { instance_name: instance_b.into() }));
        assert!(registry.apply(resolved("peer-c", instance_c, "c.local.", endpoint_c, now)));
        let snapshot = registry.snapshot();
        assert!(snapshot.peer("peer-b").is_none());
        assert!(snapshot.peer("peer-c").is_some());
    }

    #[test]
    fn full_peer_advertisement_set_rejects_new_instances_but_refreshes_existing_ones() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::new(limits(2, 2, 2, 4));
        let first = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 9000));
        let second = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 2), 9000));
        let refreshed = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 3), 9000));
        let rejected = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 4), 9000));
        let first_instance = "a-one._fjarsyn._tcp.local.";
        let second_instance = "a-two._fjarsyn._tcp.local.";
        let third_instance = "a-three._fjarsyn._tcp.local.";

        assert!(registry.apply(resolved("peer-a", first_instance, "one.local.", first, now)));
        assert!(registry.apply(resolved("peer-a", second_instance, "two.local.", second, now)));
        let full_revision = registry.snapshot().revision();

        assert!(!registry.apply(resolved(
            "peer-a",
            third_instance,
            "three.local.",
            rejected,
            now + Duration::from_secs(3),
        )));
        assert_eq!(registry.snapshot().revision(), full_revision);

        assert!(registry.apply(resolved(
            "peer-a",
            "A-TWO._FJARSYN._TCP.LOCAL.",
            "two-new.local.",
            refreshed,
            now + Duration::from_secs(2),
        )));
        let peer = registry.snapshot().peer("peer-a").unwrap().clone();
        assert_eq!(peer.advertisements.len(), 2);
        assert_eq!(peer.instance_name, "A-TWO._FJARSYN._TCP.LOCAL.");
        assert_eq!(peer.endpoints.as_ref(), &[first, refreshed]);

        assert!(
            registry.apply(PresenceObservation::Removed { instance_name: first_instance.into() })
        );
        assert!(registry.apply(resolved(
            "peer-a",
            third_instance,
            "three.local.",
            rejected,
            now + Duration::from_secs(3),
        )));
        assert_eq!(registry.snapshot().peer("peer-a").unwrap().advertisements.len(), 2);
    }

    #[test]
    fn rejected_instance_reassignment_is_atomic() {
        let now = Instant::now();
        let mut registry = PresenceRegistry::new(limits(2, 1, 1, 1));
        let endpoint_a = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 9000));
        let endpoint_b = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 2), 9000));
        let instance_a = "a._fjarsyn._tcp.local.";
        let instance_b = "b._fjarsyn._tcp.local.";

        assert!(registry.apply(resolved("peer-a", instance_a, "a.local.", endpoint_a, now)));
        assert!(registry.apply(resolved("peer-b", instance_b, "b.local.", endpoint_b, now)));
        let full_revision = registry.snapshot().revision();

        assert!(!registry.apply(resolved(
            "peer-b",
            instance_a,
            "moved.local.",
            endpoint_a,
            now + Duration::from_secs(1),
        )));
        let snapshot = registry.snapshot();
        assert_eq!(snapshot.revision(), full_revision);
        assert_eq!(snapshot.peer("peer-a").unwrap().instance_name, instance_a);
        assert_eq!(snapshot.peer("peer-b").unwrap().instance_name, instance_b);
    }

    #[test]
    fn adversarial_observation_flood_never_exceeds_cardinality_limits() {
        let now = Instant::now();
        let limits = limits(3, 2, 3, 4);
        let mut registry = PresenceRegistry::new(limits);

        for peer in 0..40 {
            for advertisement in 0..10 {
                let endpoints = (0..20)
                    .map(|endpoint| {
                        SocketAddr::from((
                            Ipv4Addr::new(10, peer as u8, advertisement as u8, endpoint as u8 + 1),
                            9000,
                        ))
                    })
                    .collect();
                registry.apply(resolved_with_endpoints(
                    &format!("peer-{peer}"),
                    &format!("peer-{peer}-{advertisement}._fjarsyn._tcp.local."),
                    "flood.local.",
                    endpoints,
                    now + Duration::from_millis((peer * 10 + advertisement) as u64),
                ));
            }
        }

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.peers().len(), limits.max_peers);
        for peer in snapshot.peers() {
            assert!(peer.advertisements.len() <= limits.max_advertisements_per_peer);
            assert!(peer.endpoints.len() <= limits.max_endpoints_per_peer);
            for advertisement in peer.advertisements.iter() {
                assert!(advertisement.endpoints.len() <= limits.max_endpoints_per_advertisement);
            }
        }
    }
}

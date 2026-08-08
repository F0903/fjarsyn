use std::collections::{HashMap, HashSet};

use super::{
    super::{Limits, NearbyAdvertisement, NearbyPeer, Snapshot},
    Observation, ResolvedAdvertisement, normalize_endpoint_hints,
};
use crate::identity::PeerId;

#[derive(Debug)]
pub(in crate::presence) struct Registry {
    by_instance: HashMap<String, ResolvedAdvertisement>,
    revision: u64,
    limits: Limits,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}

impl Registry {
    pub(in crate::presence) fn new(limits: Limits) -> Self {
        debug_assert!(limits.zero_limit().is_none());
        Self { by_instance: HashMap::new(), revision: 0, limits }
    }

    pub(in crate::presence) fn apply(&mut self, observation: Observation) -> bool {
        let changed = match observation {
            Observation::Resolved(resolved) => self.upsert(resolved),
            Observation::Removed { instance_name } => {
                self.by_instance.remove(&instance_key(&instance_name)).is_some()
            }
        };

        if changed {
            self.revision = self.revision.wrapping_add(1);
        }

        changed
    }

    pub(in crate::presence) fn clear(&mut self) -> bool {
        if self.by_instance.is_empty() {
            return false;
        }

        self.by_instance.clear();
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub(in crate::presence) fn snapshot(&self) -> Snapshot {
        let mut grouped = HashMap::<PeerId, Vec<NearbyAdvertisement>>::new();
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

        Snapshot::new(self.revision, peers.into())
    }

    fn upsert(&mut self, mut resolved: ResolvedAdvertisement) -> bool {
        if resolved.advertisement.instance_name.trim().is_empty()
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

    fn can_admit(&self, replacing_key: &str, target_peer_id: &PeerId) -> bool {
        let mut peers_without_replaced_instance = HashSet::new();
        let mut target_advertisements = 0;

        for (key, resolved) in &self.by_instance {
            if key == replacing_key {
                continue;
            }

            peers_without_replaced_instance.insert(resolved.peer_id.clone());
            if &resolved.peer_id == target_peer_id {
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

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
        time::{Duration, Instant},
    };

    use super::*;

    fn peer_id(value: &str) -> PeerId {
        PeerId::new(value).unwrap()
    }

    fn resolved(
        peer_id: &str,
        instance_name: &str,
        hostname: &str,
        endpoint: SocketAddr,
        last_seen: Instant,
    ) -> Observation {
        resolved_with_endpoints(peer_id, instance_name, hostname, vec![endpoint], last_seen)
    }

    fn resolved_with_endpoints(
        peer_id: &str,
        instance_name: &str,
        hostname: &str,
        endpoints: Vec<SocketAddr>,
        last_seen: Instant,
    ) -> Observation {
        Observation::Resolved(ResolvedAdvertisement {
            peer_id: PeerId::new(peer_id).unwrap(),
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
    ) -> Limits {
        Limits {
            max_peers,
            max_advertisements_per_peer,
            max_endpoints_per_advertisement,
            max_endpoints_per_peer,
        }
    }

    #[test]
    fn aggregates_multiple_advertisements_for_one_peer() {
        let now = Instant::now();
        let mut registry = Registry::default();
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
        let peer = snapshot.peer(&peer_id("peer-a")).unwrap();
        assert_eq!(snapshot.peers().len(), 1);
        assert_eq!(peer.advertisements.len(), 2);
        assert_eq!(peer.instance_name, "a-two._fjarsyn._tcp.local.");
        assert_eq!(peer.endpoints.as_ref(), &[second, first]);
        assert_eq!(snapshot.endpoint_hints(&peer_id("peer-a")).as_ref(), &[second, first]);
        assert!(snapshot.endpoint_hints(&peer_id("unknown-peer")).is_empty());
    }

    #[test]
    fn removal_only_removes_the_matching_advertisement() {
        let now = Instant::now();
        let mut registry = Registry::default();
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

        assert!(
            registry
                .apply(Observation::Removed { instance_name: "A-TWO._FJARSYN._TCP.LOCAL.".into() })
        );

        let peer = registry.snapshot().peer(&peer_id("peer-a")).unwrap().clone();
        assert_eq!(peer.advertisements.len(), 1);
        assert_eq!(peer.instance_name, "a-one._fjarsyn._tcp.local.");
        assert_eq!(peer.endpoints.as_ref(), &[first]);
    }

    #[test]
    fn resolving_an_existing_instance_replaces_only_that_instance() {
        let now = Instant::now();
        let mut registry = Registry::default();
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

        let peer = registry.snapshot().peer(&peer_id("peer-a")).unwrap().clone();
        assert_eq!(peer.advertisements.len(), 1);
        assert_eq!(peer.hostname, "new.local.");
        assert_eq!(peer.endpoints.as_ref(), &[new_endpoint]);
    }

    #[test]
    fn endpoint_limits_are_deduplicated_sorted_and_applied_at_both_levels() {
        let now = Instant::now();
        let mut registry = Registry::new(limits(2, 2, 2, 3));
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
        let peer = snapshot.peer(&peer_id("peer-a")).unwrap();
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
        let mut registry = Registry::new(limits(2, 2, 2, 2));
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
        let single_peer = single.peer(&peer_id("peer-a")).unwrap();
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
        let aggregate_peer = aggregate.peer(&peer_id("peer-a")).unwrap();
        assert_eq!(aggregate_peer.endpoints.as_ref(), &[ipv4_one, scoped_ipv6]);
        assert!(matches!(
            aggregate_peer.endpoints[1],
            SocketAddr::V6(address) if address.scope_id() == 23
        ));
    }

    #[test]
    fn unusable_only_advertisements_are_rejected_without_consuming_capacity() {
        let now = Instant::now();
        let mut registry = Registry::new(limits(1, 1, 1, 1));
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
        assert_eq!(
            registry.snapshot().peer(&peer_id("peer-a")).unwrap().endpoints.as_ref(),
            &[usable]
        );
    }

    #[test]
    fn full_peer_registry_rejects_newcomers_but_refresh_and_removal_free_capacity() {
        let now = Instant::now();
        let mut registry = Registry::new(limits(2, 2, 2, 4));
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
        assert!(registry.snapshot().peer(&peer_id("peer-c")).is_none());

        assert!(registry.apply(resolved(
            "peer-a",
            "A._FJARSYN._TCP.LOCAL.",
            "a-new.local.",
            endpoint_a_refreshed,
            now + Duration::from_secs(1),
        )));
        assert_eq!(
            registry.snapshot().peer(&peer_id("peer-a")).unwrap().endpoints.as_ref(),
            &[endpoint_a_refreshed]
        );

        assert!(registry.apply(Observation::Removed { instance_name: instance_b.into() }));
        assert!(registry.apply(resolved("peer-c", instance_c, "c.local.", endpoint_c, now)));
        let snapshot = registry.snapshot();
        assert!(snapshot.peer(&peer_id("peer-b")).is_none());
        assert!(snapshot.peer(&peer_id("peer-c")).is_some());
    }

    #[test]
    fn full_peer_advertisement_set_rejects_new_instances_but_refreshes_existing_ones() {
        let now = Instant::now();
        let mut registry = Registry::new(limits(2, 2, 2, 4));
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
        let peer = registry.snapshot().peer(&peer_id("peer-a")).unwrap().clone();
        assert_eq!(peer.advertisements.len(), 2);
        assert_eq!(peer.instance_name, "A-TWO._FJARSYN._TCP.LOCAL.");
        assert_eq!(peer.endpoints.as_ref(), &[first, refreshed]);

        assert!(registry.apply(Observation::Removed { instance_name: first_instance.into() }));
        assert!(registry.apply(resolved(
            "peer-a",
            third_instance,
            "three.local.",
            rejected,
            now + Duration::from_secs(3),
        )));
        assert_eq!(registry.snapshot().peer(&peer_id("peer-a")).unwrap().advertisements.len(), 2);
    }

    #[test]
    fn rejected_instance_reassignment_is_atomic() {
        let now = Instant::now();
        let mut registry = Registry::new(limits(2, 1, 1, 1));
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
        assert_eq!(snapshot.peer(&peer_id("peer-a")).unwrap().instance_name, instance_a);
        assert_eq!(snapshot.peer(&peer_id("peer-b")).unwrap().instance_name, instance_b);
    }

    #[test]
    fn adversarial_observation_flood_never_exceeds_cardinality_limits() {
        let now = Instant::now();
        let limits = limits(3, 2, 3, 4);
        let mut registry = Registry::new(limits);

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

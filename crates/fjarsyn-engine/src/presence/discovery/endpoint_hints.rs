use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
};

/// Filters, deterministically orders, de-duplicates and caps endpoint hints.
///
/// When both IP families are available and `limit` permits at least two
/// entries, the final retained slot is reserved for the otherwise absent
/// family. This prevents the IPv4-first preference order from erasing every
/// IPv6 fallback while retaining stable ordering within each family.
pub(super) fn normalize_endpoint_hints(endpoints: &mut Vec<SocketAddr>, limit: usize) {
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

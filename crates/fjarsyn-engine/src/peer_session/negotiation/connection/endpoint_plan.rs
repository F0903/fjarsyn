use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
};

use crate::peer_session::NetworkScope;

pub(super) fn plan_endpoint_hints(
    endpoint_hints: &[SocketAddr],
    network_scope: NetworkScope,
    max_attempts: usize,
) -> Vec<SocketAddr> {
    let limit = max_attempts.max(1);
    let mut seen = HashSet::with_capacity(endpoint_hints.len().min(limit.saturating_add(1)));
    let mut planned = Vec::with_capacity(endpoint_hints.len().min(limit));
    for endpoint in endpoint_hints
        .iter()
        .copied()
        .filter_map(normalize_endpoint_hint)
        .filter(|endpoint| network_scope.allows(endpoint.ip()))
    {
        if !seen.insert(endpoint) {
            continue;
        }
        if planned.len() < limit {
            planned.push(endpoint);
            if planned.len() < limit {
                continue;
            }
            if limit == 1
                || planned.iter().any(|candidate| candidate.is_ipv4() != planned[0].is_ipv4())
            {
                break;
            }
            // If the capped prefix contains only one address family, keep
            // scanning for one hint from the other family. Unauthenticated
            // claims in one family must not crowd the other family out of the
            // bounded attempt set entirely.
            continue;
        }

        if endpoint.is_ipv4() != planned[0].is_ipv4() {
            planned[limit - 1] = endpoint;
            break;
        }
    }
    planned
}

fn normalize_endpoint_hint(endpoint: SocketAddr) -> Option<SocketAddr> {
    if endpoint.port() == 0 {
        return None;
    }

    let endpoint = match endpoint {
        SocketAddr::V6(address) => match address.ip().to_ipv4_mapped() {
            Some(address) => SocketAddr::new(IpAddr::V4(address), endpoint.port()),
            None => SocketAddr::V6(address),
        },
        endpoint => endpoint,
    };
    match endpoint.ip() {
        IpAddr::V4(address)
            if address.is_unspecified() || address.is_multicast() || address.is_broadcast() =>
        {
            None
        }
        IpAddr::V6(address) if address.is_unspecified() || address.is_multicast() => None,
        _ => Some(endpoint),
    }
}

pub(in crate::peer_session::negotiation) fn secure_websocket_url(endpoint: SocketAddr) -> String {
    match endpoint {
        SocketAddr::V6(address) if address.scope_id() != 0 => {
            format!("wss://[{}%25{}]:{}/session", address.ip(), address.scope_id(), address.port())
        }
        endpoint => format!("wss://{endpoint}/session"),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV6};

    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    use super::*;

    #[test]
    fn plan_is_stable_deduplicated_capped_and_rejects_unusable_hints() {
        let first = SocketAddr::from((Ipv4Addr::LOCALHOST, 9000));
        let mapped = SocketAddr::V6(SocketAddrV6::new(
            Ipv4Addr::LOCALHOST.to_ipv6_mapped(),
            first.port(),
            0,
            0,
        ));
        let second = SocketAddr::from(([192, 168, 1, 10], 9000));
        let third = SocketAddr::from((Ipv6Addr::LOCALHOST, 9000));
        let hints = [SocketAddr::from(([0, 0, 0, 0], 9000)), first, mapped, first, second, third];

        assert_eq!(plan_endpoint_hints(&hints, NetworkScope::AllInterfaces, 2), vec![first, third]);
        assert_eq!(
            plan_endpoint_hints(&[first, second], NetworkScope::AllInterfaces, 2),
            vec![first, second]
        );
        assert_eq!(
            plan_endpoint_hints(&[first], NetworkScope::AllInterfaces, usize::MAX),
            vec![first]
        );
    }

    #[test]
    fn loopback_scope_rejects_every_non_loopback_hint() {
        let ipv4 = SocketAddr::from((Ipv4Addr::LOCALHOST, 9000));
        let ipv6 = SocketAddr::from((Ipv6Addr::LOCALHOST, 9000));
        let hints = [
            SocketAddr::from(([203, 0, 113, 1], 9000)),
            ipv4,
            SocketAddr::from(([192, 168, 1, 10], 9000)),
            ipv6,
        ];

        assert_eq!(
            plan_endpoint_hints(&hints, NetworkScope::LoopbackOnly, usize::MAX),
            vec![ipv4, ipv6]
        );
    }

    #[test]
    fn scoped_ipv6_websocket_url_is_valid_and_retains_the_zone_index() {
        let endpoint =
            SocketAddr::V6(SocketAddrV6::new("fe80::1234".parse().unwrap(), 9000, 0, 17));
        let url = secure_websocket_url(endpoint);

        assert_eq!(url, "wss://[fe80::1234%2517]:9000/session");
        assert!(url.into_client_request().is_ok());
    }
}

use std::{
    collections::HashMap,
    net::{Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use mdns_sd::{
    DaemonStatus, Receiver, ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent, ServiceInfo,
    UnregisterStatus,
};
use tokio::time::timeout;

use super::{
    model::{
        NearbyAdvertisement, PresenceObservation, ResolvedAdvertisement, normalize_endpoint_hints,
    },
    service::{PresenceBackend, PresenceServiceConfig, PresenceServiceError},
};

pub(crate) const SERVICE_TYPE: &str = "_fjarsyn._tcp.local.";
const PEER_ID_PROPERTY: &str = "peer_id";
const CLEANUP_ACKNOWLEDGEMENT_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct MdnsBackend {
    daemon: ServiceDaemon,
    events: Receiver<ServiceEvent>,
    advertised_fullname: String,
    max_endpoints_per_advertisement: usize,
    stopped: bool,
}

impl MdnsBackend {
    pub fn start(config: &PresenceServiceConfig) -> Result<Self, PresenceServiceError> {
        let daemon = ServiceDaemon::new().map_err(PresenceServiceError::CreateDaemon)?;
        let instance_name =
            config.instance_name.clone().unwrap_or_else(|| format!("fjarsyn-{}", config.peer_id));
        let hostname =
            config.hostname.clone().unwrap_or_else(|| format!("{}.local.", instance_name));

        let mut properties = HashMap::new();
        properties.insert(PEER_ID_PROPERTY.to_string(), config.peer_id.clone());

        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &hostname,
            "",
            config.signaling_port,
            Some(properties),
        )
        .map_err(PresenceServiceError::CreateAdvertisement)?
        .enable_addr_auto();
        let advertised_fullname = service.get_fullname().to_string();

        daemon.register(service).map_err(PresenceServiceError::Advertise)?;
        let events = match daemon.browse(SERVICE_TYPE) {
            Ok(events) => events,
            Err(source) => {
                let _ = daemon.unregister(&advertised_fullname);
                let _ = daemon.shutdown();
                return Err(PresenceServiceError::Browse(source));
            }
        };

        Ok(Self {
            daemon,
            events,
            advertised_fullname,
            max_endpoints_per_advertisement: config.limits.max_endpoints_per_advertisement,
            stopped: false,
        })
    }

    fn begin_stop(&mut self) -> PendingCleanup {
        if self.stopped {
            return PendingCleanup::default();
        }
        self.stopped = true;

        // Attempt every cleanup operation even when an earlier one fails. The
        // first error remains the most useful description for the caller.
        let mut result = Ok(());
        if let Err(source) = self.daemon.stop_browse(SERVICE_TYPE) {
            result = Err(PresenceServiceError::StopBrowse(source));
        }
        let advertisement = match self.daemon.unregister(&self.advertised_fullname) {
            Ok(receiver) => Some(receiver),
            Err(source) => {
                if result.is_ok() {
                    result = Err(PresenceServiceError::WithdrawAdvertisement(source));
                }
                None
            }
        };
        let daemon = match self.daemon.shutdown() {
            Ok(receiver) => Some(receiver),
            Err(source) => {
                if result.is_ok() {
                    result = Err(PresenceServiceError::ShutdownDaemon(source));
                }
                None
            }
        };

        PendingCleanup { result, advertisement, daemon }
    }

    async fn stop(&mut self) -> Result<(), PresenceServiceError> {
        self.begin_stop().finish().await
    }

    fn resolved_observation(
        info: &ResolvedService,
        max_endpoints_per_advertisement: usize,
    ) -> Option<PresenceObservation> {
        let peer_id = info.get_property_val_str(PEER_ID_PROPERTY)?.trim();
        if peer_id.is_empty() {
            return None;
        }

        let mut endpoints = info
            .get_addresses()
            .iter()
            .filter_map(|address| endpoint_from_scoped_ip(address, info.get_port()))
            .collect::<Vec<_>>();
        normalize_endpoint_hints(&mut endpoints, max_endpoints_per_advertisement);
        if endpoints.is_empty() {
            return None;
        }

        Some(PresenceObservation::Resolved(ResolvedAdvertisement {
            peer_id: peer_id.to_string(),
            advertisement: NearbyAdvertisement {
                instance_name: info.get_fullname().to_string(),
                hostname: info.get_hostname().to_string(),
                endpoints: Arc::from(endpoints),
                last_seen: Instant::now(),
            },
        }))
    }
}

#[async_trait]
impl PresenceBackend for MdnsBackend {
    async fn next_observation(
        &mut self,
    ) -> Result<Option<PresenceObservation>, PresenceServiceError> {
        loop {
            let event = match self.events.recv_async().await {
                Ok(event) => event,
                Err(_) => return Ok(None),
            };

            match event {
                ServiceEvent::ServiceResolved(info) => {
                    if let Some(observation) = Self::resolved_observation(
                        info.as_ref(),
                        self.max_endpoints_per_advertisement,
                    ) {
                        return Ok(Some(observation));
                    }
                }
                ServiceEvent::ServiceRemoved(_, instance_name) => {
                    return Ok(Some(PresenceObservation::Removed { instance_name }));
                }
                _ => {}
            }
        }
    }

    async fn shutdown(&mut self) -> Result<(), PresenceServiceError> {
        self.stop().await
    }
}

fn endpoint_from_scoped_ip(address: &ScopedIp, port: u16) -> Option<SocketAddr> {
    match address {
        ScopedIp::V4(address) => Some(SocketAddr::from((*address.addr(), port))),
        ScopedIp::V6(address) => {
            Some(scoped_ipv6_endpoint(*address.addr(), port, address.scope_id().index))
        }
        // `ScopedIp` is non-exhaustive so future address families are ignored
        // until their endpoint representation is defined deliberately.
        _ => None,
    }
}

fn scoped_ipv6_endpoint(address: Ipv6Addr, port: u16, scope_id: u32) -> SocketAddr {
    SocketAddr::V6(SocketAddrV6::new(address, port, 0, scope_id))
}

impl Drop for MdnsBackend {
    fn drop(&mut self) {
        // Drop cannot wait for daemon acknowledgements. It still queues every
        // cleanup command; explicit PresenceService::shutdown is the
        // deterministic, awaited teardown path.
        if let Err(error) = self.begin_stop().result {
            tracing::debug!("Failed to stop mDNS presence backend cleanly: {}", error);
        }
    }
}

struct PendingCleanup {
    result: Result<(), PresenceServiceError>,
    advertisement: Option<Receiver<UnregisterStatus>>,
    daemon: Option<Receiver<DaemonStatus>>,
}

impl Default for PendingCleanup {
    fn default() -> Self {
        Self { result: Ok(()), advertisement: None, daemon: None }
    }
}

impl PendingCleanup {
    async fn finish(mut self) -> Result<(), PresenceServiceError> {
        if let Some(receiver) = self.advertisement.take()
            && acknowledgement(receiver).await.is_err()
            && self.result.is_ok()
        {
            self.result = Err(PresenceServiceError::CleanupNotAcknowledged {
                operation: "advertisement withdrawal",
            });
        }

        if let Some(receiver) = self.daemon.take()
            && acknowledgement(receiver).await.is_err()
            && self.result.is_ok()
        {
            self.result =
                Err(PresenceServiceError::CleanupNotAcknowledged { operation: "daemon shutdown" });
        }

        self.result
    }
}

async fn acknowledgement<T>(receiver: Receiver<T>) -> Result<(), ()> {
    timeout(CLEANUP_ACKNOWLEDGEMENT_TIMEOUT, receiver.recv_async())
        .await
        .map_err(|_| ())?
        .map(|_| ())
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use std::net::Ipv6Addr;

    use super::*;

    #[test]
    fn scoped_ipv6_conversion_preserves_interface_index() {
        let address = "fe80::1234".parse::<Ipv6Addr>().unwrap();
        let endpoint = scoped_ipv6_endpoint(address, 9_000, 17);

        assert_eq!(endpoint, SocketAddr::V6(SocketAddrV6::new(address, 9_000, 0, 17)));
    }
}

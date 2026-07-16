use std::{fmt, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

use super::{
    mdns::MdnsBackend,
    model::{NearbyPeer, PresenceLimits, PresenceObservation, PresenceRegistry, PresenceSnapshot},
};
use crate::peer_session::{PeerEndpointResolver, PeerId, PeerSessionError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceServiceConfig {
    pub peer_id: String,
    pub signaling_port: u16,
    pub instance_name: Option<String>,
    pub hostname: Option<String>,
    pub limits: PresenceLimits,
}

impl PresenceServiceConfig {
    pub fn new(peer_id: impl Into<String>, signaling_port: u16) -> Self {
        Self {
            peer_id: peer_id.into(),
            signaling_port,
            instance_name: None,
            hostname: None,
            limits: PresenceLimits::default(),
        }
    }

    pub fn with_instance_name(mut self, instance_name: impl Into<String>) -> Self {
        self.instance_name = Some(instance_name.into());
        self
    }

    pub fn with_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    pub fn with_limits(mut self, limits: PresenceLimits) -> Self {
        self.limits = limits;
        self
    }

    fn validate(&self) -> Result<(), PresenceServiceError> {
        if self.peer_id.trim().is_empty() {
            return Err(PresenceServiceError::InvalidPeerId);
        }
        if self.signaling_port == 0 {
            return Err(PresenceServiceError::InvalidSignalingPort);
        }
        if self.instance_name.as_ref().is_some_and(|value| value.trim().is_empty()) {
            return Err(PresenceServiceError::InvalidInstanceName);
        }
        if self.hostname.as_ref().is_some_and(|value| value.trim().is_empty()) {
            return Err(PresenceServiceError::InvalidHostname);
        }
        if let Some(name) = self.limits.zero_limit() {
            return Err(PresenceServiceError::InvalidLimit { name });
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PresenceServiceError {
    #[error("peer ID cannot be empty")]
    InvalidPeerId,
    #[error("signaling port cannot be zero")]
    InvalidSignalingPort,
    #[error("mDNS instance name cannot be empty")]
    InvalidInstanceName,
    #[error("mDNS hostname cannot be empty")]
    InvalidHostname,
    #[error("presence limit {name} must be greater than zero")]
    InvalidLimit { name: &'static str },
    #[error("failed to create the mDNS daemon: {0}")]
    CreateDaemon(#[source] mdns_sd::Error),
    #[error("failed to create the local mDNS advertisement: {0}")]
    CreateAdvertisement(#[source] mdns_sd::Error),
    #[error("failed to advertise local presence: {0}")]
    Advertise(#[source] mdns_sd::Error),
    #[error("failed to browse for nearby peers: {0}")]
    Browse(#[source] mdns_sd::Error),
    #[error("failed to stop browsing for nearby peers: {0}")]
    StopBrowse(#[source] mdns_sd::Error),
    #[error("failed to withdraw the local presence advertisement: {0}")]
    WithdrawAdvertisement(#[source] mdns_sd::Error),
    #[error("failed to stop the mDNS daemon: {0}")]
    ShutdownDaemon(#[source] mdns_sd::Error),
    #[error("mDNS cleanup did not acknowledge {operation}")]
    CleanupNotAcknowledged { operation: &'static str },
    #[error("presence worker task failed: {0}")]
    WorkerJoin(#[source] tokio::task::JoinError),
}

/// Cloneable, read-only access to live presence snapshots.
///
/// The handle intentionally exposes no connection operation: mDNS only
/// provides unauthenticated reachability hints.
#[derive(Clone)]
pub struct PresenceHandle {
    snapshots: watch::Receiver<PresenceSnapshot>,
}

impl PresenceHandle {
    pub fn snapshot(&self) -> PresenceSnapshot {
        self.snapshots.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<PresenceSnapshot> {
        self.snapshots.clone()
    }

    pub fn nearby_peer(&self, peer_id: &str) -> Option<NearbyPeer> {
        self.snapshots.borrow().peer(peer_id).cloned()
    }

    /// Returns all current endpoint hints for `peer_id`.
    ///
    /// The returned addresses are unauthenticated mDNS data. Backend
    /// composition must verify the expected trusted identity while establishing
    /// a session and must never treat endpoint selection as identity proof.
    pub fn endpoint_hints(&self, peer_id: &str) -> Arc<[SocketAddr]> {
        self.snapshots.borrow().endpoint_hints(peer_id)
    }
}

#[async_trait]
impl PeerEndpointResolver for PresenceHandle {
    async fn endpoint_hints_for(
        &self,
        peer_id: &PeerId,
    ) -> Result<Arc<[SocketAddr]>, PeerSessionError> {
        Ok(self.endpoint_hints(peer_id.as_str()))
    }
}

impl fmt::Debug for PresenceHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PresenceHandle")
            .field("snapshot", &*self.snapshots.borrow())
            .finish()
    }
}

/// Owner of the mDNS presence worker.
///
/// Keep this object in the application runtime and distribute only
/// [`PresenceHandle`] clones. [`PresenceService::shutdown`] is the normal
/// teardown path and waits for the worker to withdraw its advertisement and
/// stop its mDNS daemon.
pub struct PresenceService {
    handle: PresenceHandle,
    shutdown_tx: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<Result<(), PresenceServiceError>>>,
}

impl PresenceService {
    pub fn start(config: PresenceServiceConfig) -> Result<Self, PresenceServiceError> {
        config.validate()?;
        let backend = MdnsBackend::start(&config)?;
        Ok(Self::start_with_backend(config.peer_id, config.limits, Box::new(backend)))
    }

    pub fn handle(&self) -> PresenceHandle {
        self.handle.clone()
    }

    pub fn snapshot(&self) -> PresenceSnapshot {
        self.handle.snapshot()
    }

    pub fn subscribe(&self) -> watch::Receiver<PresenceSnapshot> {
        self.handle.subscribe()
    }

    pub fn endpoint_hints(&self, peer_id: &str) -> Arc<[SocketAddr]> {
        self.handle.endpoint_hints(peer_id)
    }

    pub async fn shutdown(mut self) -> Result<(), PresenceServiceError> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        match self.worker.take() {
            Some(worker) => worker.await.map_err(PresenceServiceError::WorkerJoin)?,
            None => Ok(()),
        }
    }

    fn start_with_backend(
        local_peer_id: String,
        limits: PresenceLimits,
        backend: Box<dyn PresenceBackend>,
    ) -> Self {
        let (snapshot_tx, snapshot_rx) = watch::channel(PresenceSnapshot::default());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let worker =
            tokio::spawn(run_worker(local_peer_id, limits, backend, snapshot_tx, shutdown_rx));

        Self {
            handle: PresenceHandle { snapshots: snapshot_rx },
            shutdown_tx: Some(shutdown_tx),
            worker: Some(worker),
        }
    }
}

impl fmt::Debug for PresenceService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PresenceService")
            .field("snapshot", &self.snapshot())
            .field("worker_finished", &self.worker.as_ref().is_none_or(JoinHandle::is_finished))
            .finish()
    }
}

impl Drop for PresenceService {
    fn drop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(worker) = self.worker.take() {
            // Explicit shutdown is preferred because it awaits cleanup. Abort is
            // a safety net; dropping MdnsBackend still sends every daemon cleanup
            // command synchronously.
            worker.abort();
        }
    }
}

#[async_trait]
pub(crate) trait PresenceBackend: Send {
    async fn next_observation(
        &mut self,
    ) -> Result<Option<PresenceObservation>, PresenceServiceError>;

    async fn shutdown(&mut self) -> Result<(), PresenceServiceError>;
}

async fn run_worker(
    local_peer_id: String,
    limits: PresenceLimits,
    mut backend: Box<dyn PresenceBackend>,
    snapshot_tx: watch::Sender<PresenceSnapshot>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), PresenceServiceError> {
    let mut registry = PresenceRegistry::new(limits);

    let run_result = loop {
        tokio::select! {
            biased;
            _ = &mut shutdown_rx => break Ok(()),
            observation = backend.next_observation() => {
                match observation {
                    Ok(Some(PresenceObservation::Resolved(resolved)))
                        if resolved.peer_id == local_peer_id => {}
                    Ok(Some(observation)) => {
                        if registry.apply(observation) {
                            snapshot_tx.send_replace(registry.snapshot());
                        }
                    }
                    Ok(None) => break Ok(()),
                    Err(error) => break Err(error),
                }
            }
        }
    };

    if registry.clear() {
        snapshot_tx.send_replace(registry.snapshot());
    }

    let shutdown_result = backend.shutdown().await;
    match (run_result, shutdown_result) {
        (Err(error), _) => Err(error),
        (Ok(()), result) => result,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, SocketAddr},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Instant,
    };

    use tokio::sync::mpsc;

    use super::*;
    use crate::presence::model::{NearbyAdvertisement, ResolvedAdvertisement};

    struct FakeBackend {
        observations: mpsc::Receiver<PresenceObservation>,
        shutdown_called: Arc<AtomicBool>,
    }

    #[async_trait]
    impl PresenceBackend for FakeBackend {
        async fn next_observation(
            &mut self,
        ) -> Result<Option<PresenceObservation>, PresenceServiceError> {
            Ok(self.observations.recv().await)
        }

        async fn shutdown(&mut self) -> Result<(), PresenceServiceError> {
            self.shutdown_called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn fake_service() -> (PresenceService, mpsc::Sender<PresenceObservation>, Arc<AtomicBool>) {
        fake_service_with_limits(PresenceLimits::default())
    }

    fn fake_service_with_limits(
        limits: PresenceLimits,
    ) -> (PresenceService, mpsc::Sender<PresenceObservation>, Arc<AtomicBool>) {
        let (observation_tx, observation_rx) = mpsc::channel(4);
        let shutdown_called = Arc::new(AtomicBool::new(false));
        let service = PresenceService::start_with_backend(
            "local-peer".into(),
            limits,
            Box::new(FakeBackend {
                observations: observation_rx,
                shutdown_called: shutdown_called.clone(),
            }),
        );
        (service, observation_tx, shutdown_called)
    }

    fn resolved_with_instance(
        peer_id: &str,
        instance_name: impl Into<String>,
        port: u16,
    ) -> PresenceObservation {
        PresenceObservation::Resolved(ResolvedAdvertisement {
            peer_id: peer_id.into(),
            advertisement: NearbyAdvertisement {
                instance_name: instance_name.into(),
                hostname: format!("{peer_id}.local."),
                endpoints: Arc::from([SocketAddr::from((Ipv4Addr::LOCALHOST, port))]),
                last_seen: Instant::now(),
            },
        })
    }

    fn resolved(peer_id: &str) -> PresenceObservation {
        resolved_with_instance(peer_id, format!("{peer_id}._fjarsyn._tcp.local."), 9000)
    }

    #[tokio::test]
    async fn publishes_observations_and_exposes_endpoint_hints() {
        let (service, observations, _) = fake_service();
        let mut snapshots = service.subscribe();

        observations.send(resolved("peer-a")).await.unwrap();
        snapshots.changed().await.unwrap();

        assert!(snapshots.borrow().is_nearby("peer-a"));
        assert_eq!(
            service.endpoint_hints("peer-a").as_ref(),
            &[SocketAddr::from((Ipv4Addr::LOCALHOST, 9000))]
        );
        assert!(service.endpoint_hints("unknown-peer").is_empty());
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn ignores_unauthenticated_presence_claiming_the_local_peer_id() {
        let (service, observations, _) = fake_service();
        let mut snapshots = service.subscribe();
        observations.send(resolved("local-peer")).await.unwrap();
        observations.send(resolved("peer-a")).await.unwrap();
        snapshots.changed().await.unwrap();

        let snapshot = service.snapshot();
        assert!(!snapshot.is_nearby("local-peer"));
        assert!(snapshot.is_nearby("peer-a"));
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn preserves_other_advertisements_when_one_is_removed() {
        let (service, observations, _) = fake_service();
        let mut snapshots = service.subscribe();
        let first_instance = "peer-a-one._fjarsyn._tcp.local.";
        let second_instance = "peer-a-two._fjarsyn._tcp.local.";

        observations.send(resolved_with_instance("peer-a", first_instance, 9000)).await.unwrap();
        observations.send(resolved_with_instance("peer-a", second_instance, 9001)).await.unwrap();

        while snapshots
            .borrow_and_update()
            .peer("peer-a")
            .is_none_or(|peer| peer.advertisements.len() < 2)
        {
            snapshots.changed().await.unwrap();
        }

        observations
            .send(PresenceObservation::Removed { instance_name: first_instance.into() })
            .await
            .unwrap();
        while snapshots.borrow_and_update().revision() < 3 {
            snapshots.changed().await.unwrap();
        }

        let snapshot = snapshots.borrow();
        let peer = snapshot.peer("peer-a").unwrap();
        assert_eq!(peer.advertisements.len(), 1);
        assert_eq!(peer.instance_name, second_instance);
        assert_eq!(peer.endpoints.as_ref(), &[SocketAddr::from((Ipv4Addr::LOCALHOST, 9001))]);
        drop(snapshot);
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn worker_applies_configured_registry_limits() {
        let limits = PresenceLimits {
            max_peers: 2,
            max_advertisements_per_peer: 1,
            max_endpoints_per_advertisement: 1,
            max_endpoints_per_peer: 1,
        };
        let (service, observations, _) = fake_service_with_limits(limits);
        let mut snapshots = service.subscribe();

        observations.send(resolved("peer-a")).await.unwrap();
        observations.send(resolved("peer-b")).await.unwrap();
        while snapshots.borrow_and_update().peers().len() < limits.max_peers {
            snapshots.changed().await.unwrap();
        }
        let full_revision = snapshots.borrow().revision();

        observations.send(resolved("peer-c")).await.unwrap();
        observations
            .send(resolved_with_instance("peer-a", "peer-a._fjarsyn._tcp.local.", 9001))
            .await
            .unwrap();
        while snapshots.borrow_and_update().revision() == full_revision {
            snapshots.changed().await.unwrap();
        }

        let snapshot = snapshots.borrow();
        assert_eq!(snapshot.peers().len(), limits.max_peers);
        assert!(snapshot.peer("peer-a").is_some());
        assert!(snapshot.peer("peer-b").is_some());
        assert!(snapshot.peer("peer-c").is_none());
        drop(snapshot);
        service.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn explicit_shutdown_clears_snapshots_and_stops_the_owned_backend() {
        let (service, observations, shutdown_called) = fake_service();
        let mut snapshots = service.subscribe();

        observations.send(resolved("peer-a")).await.unwrap();
        snapshots.changed().await.unwrap();
        assert!(snapshots.borrow_and_update().is_nearby("peer-a"));

        service.shutdown().await.unwrap();

        assert!(shutdown_called.load(Ordering::SeqCst));
        assert!(snapshots.borrow().peers().is_empty());
    }

    #[test]
    fn rejects_invalid_public_configuration() {
        assert!(matches!(
            PresenceServiceConfig::new("", 9000).validate(),
            Err(PresenceServiceError::InvalidPeerId)
        ));
        assert!(matches!(
            PresenceServiceConfig::new("peer-a", 0).validate(),
            Err(PresenceServiceError::InvalidSignalingPort)
        ));

        for (name, limits) in [
            ("max_peers", PresenceLimits { max_peers: 0, ..PresenceLimits::default() }),
            (
                "max_advertisements_per_peer",
                PresenceLimits { max_advertisements_per_peer: 0, ..PresenceLimits::default() },
            ),
            (
                "max_endpoints_per_advertisement",
                PresenceLimits { max_endpoints_per_advertisement: 0, ..PresenceLimits::default() },
            ),
            (
                "max_endpoints_per_peer",
                PresenceLimits { max_endpoints_per_peer: 0, ..PresenceLimits::default() },
            ),
        ] {
            assert!(matches!(
                PresenceServiceConfig::new("peer-a", 9000).with_limits(limits).validate(),
                Err(PresenceServiceError::InvalidLimit { name: invalid }) if invalid == name
            ));
        }
    }
}

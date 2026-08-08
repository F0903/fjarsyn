use std::{fmt, time::Duration};

use async_trait::async_trait;
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
};

use super::{
    Backend, Config, Error, Limits, MdnsBackend, Observation, Registry, ServiceHandle, Snapshot,
};
use crate::{
    identity::PeerId,
    service_host::{HostedService, ShutdownContext},
};

/// Owner of the mDNS presence worker.
///
/// The service host retains this object and distributes only
/// [`ServiceHandle`] clones. [`HostedService::shutdown`] is the normal
/// teardown path and waits for the worker to withdraw its advertisement and
/// stop its mDNS daemon.
pub struct PresenceService {
    handle: ServiceHandle,
    shutdown_tx: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<Result<(), Error>>>,
    shutdown_timeout: Duration,
}

impl PresenceService {
    pub fn start(config: Config) -> Result<Self, Error> {
        config.validate()?;
        let backend = MdnsBackend::start(&config)?;
        Ok(Self::start_with_backend(
            config.peer_id,
            config.limits,
            config.shutdown_timeout,
            Box::new(backend),
        ))
    }

    fn start_with_backend(
        local_peer_id: PeerId,
        limits: Limits,
        shutdown_timeout: Duration,
        backend: Box<dyn Backend>,
    ) -> Self {
        let (snapshot_tx, snapshot_rx) = watch::channel(Snapshot::default());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let worker =
            tokio::spawn(run_worker(local_peer_id, limits, backend, snapshot_tx, shutdown_rx));

        Self {
            handle: ServiceHandle::new(snapshot_rx),
            shutdown_tx: Some(shutdown_tx),
            worker: Some(worker),
            shutdown_timeout,
        }
    }
}

#[async_trait]
impl HostedService for PresenceService {
    const NAME: &'static str = "presence";

    type ServiceHandle = ServiceHandle;
    type Error = Error;

    fn service_handle(&self) -> Self::ServiceHandle {
        self.handle.clone()
    }

    async fn shutdown(&mut self, context: ShutdownContext) -> Result<(), Self::Error> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        let Some(worker) = self.worker.as_mut() else {
            return Ok(());
        };
        let deadline = context.bounded_deadline(self.shutdown_timeout);
        let result = match tokio::time::timeout_at(deadline, &mut *worker).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(Error::WorkerJoin(error)),
            Err(_) => {
                // The deadline is absolute. Abort cooperatively and let the
                // handle removal below detach any cleanup that cannot unwind
                // before the shared shutdown budget expires.
                worker.abort();
                Err(Error::ShutdownTimeout)
            }
        };
        self.worker.take();
        result
    }

    fn cancel(&mut self) {
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

impl fmt::Debug for PresenceService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PresenceService")
            .field("snapshot", &self.handle.snapshot())
            .field("worker_finished", &self.worker.as_ref().is_none_or(JoinHandle::is_finished))
            .finish()
    }
}

impl Drop for PresenceService {
    fn drop(&mut self) {
        self.cancel();
    }
}

async fn run_worker(
    local_peer_id: PeerId,
    limits: Limits,
    mut backend: Box<dyn Backend>,
    snapshot_tx: watch::Sender<Snapshot>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), Error> {
    let mut registry = Registry::new(limits);

    let run_result = loop {
        tokio::select! {
            biased;
            _ = &mut shutdown_rx => break Ok(()),
            observation = backend.next_observation() => {
                match observation {
                    Ok(Some(Observation::Resolved(resolved)))
                        if resolved.peer_id == local_peer_id => {}
                    Ok(Some(observation)) => {
                        if registry.apply(observation) {
                            snapshot_tx.send_replace(registry.snapshot());
                        }
                    }
                    Ok(None) => break Err(Error::ObservationStreamClosed),
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

    use async_trait::async_trait;
    use tokio::sync::{mpsc, oneshot};

    use super::*;
    use crate::presence::{NearbyAdvertisement, ResolvedAdvertisement};

    struct FakeBackend {
        observations: mpsc::Receiver<Observation>,
        shutdown_called: Arc<AtomicBool>,
        shutdown_gate: Option<oneshot::Receiver<()>>,
    }

    #[async_trait]
    impl Backend for FakeBackend {
        async fn next_observation(&mut self) -> Result<Option<Observation>, Error> {
            Ok(self.observations.recv().await)
        }

        async fn shutdown(&mut self) -> Result<(), Error> {
            if let Some(gate) = self.shutdown_gate.take() {
                let _ = gate.await;
            }
            self.shutdown_called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn fake_service() -> (PresenceService, mpsc::Sender<Observation>, Arc<AtomicBool>) {
        fake_service_with_limits(Limits::default())
    }

    fn fake_service_with_limits(
        limits: Limits,
    ) -> (PresenceService, mpsc::Sender<Observation>, Arc<AtomicBool>) {
        let (observation_tx, observation_rx) = mpsc::channel(4);
        let shutdown_called = Arc::new(AtomicBool::new(false));
        let service = PresenceService::start_with_backend(
            peer_id("local-peer"),
            limits,
            Duration::from_secs(1),
            Box::new(FakeBackend {
                observations: observation_rx,
                shutdown_called: shutdown_called.clone(),
                shutdown_gate: None,
            }),
        );
        (service, observation_tx, shutdown_called)
    }

    fn peer_id(value: &str) -> PeerId {
        PeerId::new(value).unwrap()
    }

    fn resolved_with_instance(
        peer_id: &str,
        instance_name: impl Into<String>,
        port: u16,
    ) -> Observation {
        Observation::Resolved(ResolvedAdvertisement {
            peer_id: PeerId::new(peer_id).unwrap(),
            advertisement: NearbyAdvertisement {
                instance_name: instance_name.into(),
                hostname: format!("{peer_id}.local."),
                endpoints: Arc::from([SocketAddr::from((Ipv4Addr::LOCALHOST, port))]),
                last_seen: Instant::now(),
            },
        })
    }

    fn resolved(peer_id: &str) -> Observation {
        resolved_with_instance(peer_id, format!("{peer_id}._fjarsyn._tcp.local."), 9000)
    }

    #[tokio::test]
    async fn publishes_observations_and_exposes_endpoint_hints() {
        let (mut service, observations, _) = fake_service();
        let handle = service.service_handle();
        let mut snapshots = handle.subscribe();

        observations.send(resolved("peer-a")).await.unwrap();
        snapshots.changed().await.unwrap();

        assert!(snapshots.borrow().is_nearby(&peer_id("peer-a")));
        assert_eq!(
            handle.endpoint_hints(&peer_id("peer-a")).as_ref(),
            &[SocketAddr::from((Ipv4Addr::LOCALHOST, 9000))]
        );
        assert!(handle.endpoint_hints(&peer_id("unknown-peer")).is_empty());
        service.shutdown(ShutdownContext::default()).await.unwrap();
    }

    #[tokio::test]
    async fn ignores_unauthenticated_presence_claiming_the_local_peer_id() {
        let (mut service, observations, _) = fake_service();
        let handle = service.service_handle();
        let mut snapshots = handle.subscribe();
        observations.send(resolved("local-peer")).await.unwrap();
        observations.send(resolved("peer-a")).await.unwrap();
        snapshots.changed().await.unwrap();

        let snapshot = handle.snapshot();
        assert!(!snapshot.is_nearby(&peer_id("local-peer")));
        assert!(snapshot.is_nearby(&peer_id("peer-a")));
        service.shutdown(ShutdownContext::default()).await.unwrap();
    }

    #[tokio::test]
    async fn preserves_other_advertisements_when_one_is_removed() {
        let (mut service, observations, _) = fake_service();
        let mut snapshots = service.service_handle().subscribe();
        let first_instance = "peer-a-one._fjarsyn._tcp.local.";
        let second_instance = "peer-a-two._fjarsyn._tcp.local.";

        observations.send(resolved_with_instance("peer-a", first_instance, 9000)).await.unwrap();
        observations.send(resolved_with_instance("peer-a", second_instance, 9001)).await.unwrap();

        while snapshots
            .borrow_and_update()
            .peer(&peer_id("peer-a"))
            .is_none_or(|peer| peer.advertisements.len() < 2)
        {
            snapshots.changed().await.unwrap();
        }

        observations
            .send(Observation::Removed { instance_name: first_instance.into() })
            .await
            .unwrap();
        while snapshots.borrow_and_update().revision() < 3 {
            snapshots.changed().await.unwrap();
        }

        let snapshot = snapshots.borrow();
        let peer = snapshot.peer(&peer_id("peer-a")).unwrap();
        assert_eq!(peer.advertisements.len(), 1);
        assert_eq!(peer.instance_name, second_instance);
        assert_eq!(peer.endpoints.as_ref(), &[SocketAddr::from((Ipv4Addr::LOCALHOST, 9001))]);
        drop(snapshot);
        service.shutdown(ShutdownContext::default()).await.unwrap();
    }

    #[tokio::test]
    async fn worker_applies_configured_registry_limits() {
        let limits = Limits {
            max_peers: 2,
            max_advertisements_per_peer: 1,
            max_endpoints_per_advertisement: 1,
            max_endpoints_per_peer: 1,
        };
        let (mut service, observations, _) = fake_service_with_limits(limits);
        let mut snapshots = service.service_handle().subscribe();

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
        assert!(snapshot.peer(&peer_id("peer-a")).is_some());
        assert!(snapshot.peer(&peer_id("peer-b")).is_some());
        assert!(snapshot.peer(&peer_id("peer-c")).is_none());
        drop(snapshot);
        service.shutdown(ShutdownContext::default()).await.unwrap();
    }

    #[tokio::test]
    async fn explicit_shutdown_clears_snapshots_and_stops_the_owned_backend() {
        let (mut service, observations, shutdown_called) = fake_service();
        let mut snapshots = service.service_handle().subscribe();

        observations.send(resolved("peer-a")).await.unwrap();
        snapshots.changed().await.unwrap();
        assert!(snapshots.borrow_and_update().is_nearby(&peer_id("peer-a")));

        service.shutdown(ShutdownContext::default()).await.unwrap();

        assert!(shutdown_called.load(Ordering::SeqCst));
        assert!(snapshots.borrow().peers().is_empty());
    }

    #[tokio::test]
    async fn shutdown_aborts_a_backend_that_misses_its_deadline() {
        let (_observation_tx, observation_rx) = mpsc::channel(1);
        let shutdown_called = Arc::new(AtomicBool::new(false));
        let (_gate_tx, gate_rx) = oneshot::channel();
        let mut service = PresenceService::start_with_backend(
            peer_id("local-peer"),
            Limits::default(),
            Duration::from_millis(10),
            Box::new(FakeBackend {
                observations: observation_rx,
                shutdown_called: shutdown_called.clone(),
                shutdown_gate: Some(gate_rx),
            }),
        );

        assert!(matches!(
            service.shutdown(ShutdownContext::default()).await,
            Err(Error::ShutdownTimeout)
        ));
        assert!(!shutdown_called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn unexpected_observation_stream_closure_is_reported() {
        let (mut service, observations, shutdown_called) = fake_service();
        drop(observations);

        tokio::time::timeout(Duration::from_secs(1), async {
            while !service.worker.as_ref().is_some_and(JoinHandle::is_finished) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("presence worker must observe the closed stream");

        assert!(matches!(
            service.shutdown(ShutdownContext::default()).await,
            Err(Error::ObservationStreamClosed)
        ));
        assert!(shutdown_called.load(Ordering::SeqCst));
    }

    #[test]
    fn rejects_invalid_public_configuration() {
        assert!(matches!(
            Config::new(peer_id("peer-a"), 0).validate(),
            Err(Error::InvalidSignalingPort)
        ));

        for (name, limits) in [
            ("max_peers", Limits { max_peers: 0, ..Limits::default() }),
            (
                "max_advertisements_per_peer",
                Limits { max_advertisements_per_peer: 0, ..Limits::default() },
            ),
            (
                "max_endpoints_per_advertisement",
                Limits { max_endpoints_per_advertisement: 0, ..Limits::default() },
            ),
            ("max_endpoints_per_peer", Limits { max_endpoints_per_peer: 0, ..Limits::default() }),
        ] {
            assert!(matches!(
                Config::new(peer_id("peer-a"), 9000).with_limits(limits).validate(),
                Err(Error::InvalidLimit { name: invalid }) if invalid == name
            ));
        }

        assert!(matches!(
            Config::new(peer_id("peer-a"), 9000).with_shutdown_timeout(Duration::ZERO).validate(),
            Err(Error::InvalidShutdownTimeout)
        ));
    }
}

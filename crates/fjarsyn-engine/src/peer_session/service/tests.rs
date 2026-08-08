use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::{broadcast, mpsc, oneshot};

use super::{orchestration::Command, *};
use crate::{
    identity::{LocalPeerIdentity, PeerId, TrustedPeerIdentity},
    peer_session::{
        CloseReason, EndpointResolver, Error, Event, MessageId, RemoteVideoSource, SessionId,
        TrustBarrierOwnerId, TrustedPeerResolver,
    },
    service_host::{HostedService, ShutdownContext},
};

#[derive(Debug, Default)]
struct TestDirectory {
    trusted: std::sync::RwLock<HashMap<PeerId, TrustedPeerIdentity>>,
    endpoint_hints: std::sync::RwLock<HashMap<PeerId, Arc<[SocketAddr]>>>,
}

impl TestDirectory {
    fn insert_peer(&self, peer_id: PeerId, public_key: String, endpoint: SocketAddr) {
        self.insert_peer_with_hints(peer_id, public_key, Arc::from([endpoint]));
    }

    fn insert_peer_with_hints(
        &self,
        peer_id: PeerId,
        public_key: String,
        endpoint_hints: Arc<[SocketAddr]>,
    ) {
        self.trusted
            .write()
            .unwrap()
            .insert(peer_id.clone(), TrustedPeerIdentity::new(peer_id.clone(), public_key));
        self.endpoint_hints.write().unwrap().insert(peer_id, endpoint_hints);
    }
}

#[async_trait]
impl TrustedPeerResolver for TestDirectory {
    async fn trusted_peer(&self, peer_id: &PeerId) -> Result<Option<TrustedPeerIdentity>, Error> {
        Ok(self.trusted.read().unwrap().get(peer_id).cloned())
    }
}

#[async_trait]
impl EndpointResolver for TestDirectory {
    async fn endpoint_hints_for(&self, peer_id: &PeerId) -> Result<Arc<[SocketAddr]>, Error> {
        Ok(self
            .endpoint_hints
            .read()
            .unwrap()
            .get(peer_id)
            .cloned()
            .unwrap_or_else(|| Arc::from([])))
    }
}

#[derive(Debug, Default)]
struct BlockingDirectory {
    entered: tokio::sync::Notify,
}

#[async_trait]
impl TrustedPeerResolver for BlockingDirectory {
    async fn trusted_peer(&self, _peer_id: &PeerId) -> Result<Option<TrustedPeerIdentity>, Error> {
        self.entered.notify_one();
        std::future::pending().await
    }
}

#[async_trait]
impl EndpointResolver for BlockingDirectory {
    async fn endpoint_hints_for(&self, _peer_id: &PeerId) -> Result<Arc<[SocketAddr]>, Error> {
        Ok(Arc::from([]))
    }
}

async fn start_test_pair() -> (PeerSessionService, PeerSessionService, PeerId, PeerId) {
    let peer_a = PeerId::new("peer-a").unwrap();
    let peer_b = PeerId::new("peer-b").unwrap();
    let directory_a = Arc::new(TestDirectory::default());
    let directory_b = Arc::new(TestDirectory::default());
    let mut config_a = Config::new(directory_a.clone(), directory_a.clone());
    config_a.local_peer_id = Some(peer_a.clone());
    config_a.limits.request_timeout = Duration::from_secs(5);
    config_a.limits.negotiation_timeout = Duration::from_secs(10);
    let mut config_b = Config::new(directory_b.clone(), directory_b.clone());
    config_b.local_peer_id = Some(peer_b.clone());
    config_b.limits.request_timeout = Duration::from_secs(5);
    config_b.limits.negotiation_timeout = Duration::from_secs(10);

    let service_a = PeerSessionService::start(config_a).await.unwrap();
    let service_b = PeerSessionService::start(config_b).await.unwrap();
    directory_a.insert_peer(
        peer_b.clone(),
        service_b.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
    );
    directory_b.insert_peer(
        peer_a.clone(),
        service_a.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
    );
    (service_a, service_b, peer_a, peer_b)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connect_falls_back_after_a_wrong_peer_endpoint_fails_authentication() {
    let peer_a = PeerId::new("peer-a").unwrap();
    let peer_b = PeerId::new("peer-b").unwrap();
    let peer_c = PeerId::new("peer-c").unwrap();
    let directory_a = Arc::new(TestDirectory::default());
    let directory_b = Arc::new(TestDirectory::default());
    let directory_c = Arc::new(TestDirectory::default());
    let mut config_a = Config::new(directory_a.clone(), directory_a.clone());
    config_a.local_peer_id = Some(peer_a.clone());
    config_a.limits.endpoint_attempt_timeout = Duration::from_millis(250);
    let mut config_b = Config::new(directory_b.clone(), directory_b.clone());
    config_b.local_peer_id = Some(peer_b.clone());
    let mut config_c = Config::new(directory_c.clone(), directory_c.clone());
    config_c.local_peer_id = Some(peer_c);

    let mut service_a = PeerSessionService::start(config_a).await.unwrap();
    let mut service_b = PeerSessionService::start(config_b).await.unwrap();
    let mut service_c = PeerSessionService::start(config_c).await.unwrap();
    directory_a.insert_peer_with_hints(
        peer_b.clone(),
        service_b.local_public_key(),
        Arc::from([
            SocketAddr::from(([127, 0, 0, 1], service_c.signaling_port())),
            SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
        ]),
    );
    directory_b.insert_peer(
        peer_a.clone(),
        service_a.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
    );
    directory_c.insert_peer(
        peer_a.clone(),
        service_a.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
    );

    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_b = handle_b.events();
    let mut events_c = service_c.service_handle().events();
    let session_id = handle_a.connect(peer_b).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    assert!(tokio::time::timeout(Duration::from_millis(100), events_c.recv()).await.is_err());

    tokio::time::timeout(Duration::from_secs(6), async {
        let (result_a, result_b, result_c) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
            service_c.shutdown(ShutdownContext::default()),
        );
        result_a.unwrap();
        result_b.unwrap();
        result_c.unwrap();
    })
    .await
    .expect("fallback test services did not shut down within their owner deadline");
}

#[tokio::test]
async fn exhausted_endpoint_hints_return_a_structured_error_without_a_session() {
    let local_peer = PeerId::new("local").unwrap();
    let remote_peer = PeerId::new("remote").unwrap();
    let directory = Arc::new(TestDirectory::default());
    let mut config = Config::new(directory.clone(), directory.clone());
    config.local_peer_id = Some(local_peer);
    config.limits.max_endpoint_attempts = 1;
    config.limits.endpoint_attempt_timeout = Duration::from_millis(500);
    let mut service = PeerSessionService::start(config).await.unwrap();
    let failing_listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let failing_endpoint = failing_listener.local_addr().unwrap();
    let failing_task = tokio::spawn(async move {
        let (stream, _) = failing_listener.accept().await.unwrap();
        drop(stream);
    });
    directory.insert_peer_with_hints(
        remote_peer.clone(),
        LocalPeerIdentity::generate().public_key_base64(),
        Arc::from([failing_endpoint, SocketAddr::from(([127, 0, 0, 1], 1))]),
    );

    assert_eq!(
        service.service_handle().connect(remote_peer.clone()).await,
        Err(Error::EndpointAttemptsExhausted { peer_id: remote_peer, attempted: 1 })
    );
    failing_task.await.unwrap();
    assert!(service.service_handle().snapshot().sessions.is_empty());
    service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_connect_converges_and_active_shutdown_joins_both_services() {
    let (mut service_a, mut service_b, peer_a, peer_b) = start_test_pair().await;
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();

    let (outgoing_a, outgoing_b) =
        tokio::join!(handle_a.connect(peer_b.clone()), handle_b.connect(peer_a.clone()),);
    let winning_session = outgoing_a.unwrap();
    let _superseded_session = outgoing_b.unwrap();
    wait_for_connected(&mut events_a, winning_session).await;
    wait_for_connected(&mut events_b, winning_session).await;
    assert_eq!(handle_a.snapshot().sessions.len(), 1);
    assert_eq!(handle_b.snapshot().sessions.len(), 1);
    assert_eq!(handle_a.snapshot().sessions[0].session_id, winning_session);
    assert_eq!(handle_b.snapshot().sessions[0].session_id, winning_session);

    tokio::time::timeout(Duration::from_secs(6), async {
        let (result_a, result_b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        result_a.unwrap();
        result_b.unwrap();
    })
    .await
    .expect("active peer services did not shut down within their owner deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn negotiating_sessions_shutdown_with_one_owner_deadline() {
    let (mut service_a, mut service_b, peer_a, peer_b) = start_test_pair().await;
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_b = handle_b.events();
    let session_id = handle_a.connect(peer_b).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);

    tokio::time::timeout(Duration::from_secs(6), async {
        let (result_a, result_b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        result_a.unwrap();
        result_b.unwrap();
    })
    .await
    .expect("negotiating peer services did not shut down within their owner deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn suspending_trust_closes_the_session_blocks_connect_and_can_be_resumed() {
    let (mut service_a, mut service_b, peer_a, peer_b) = start_test_pair().await;
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();
    let barrier_owner = TrustBarrierOwnerId::allocate();

    let session_id = handle_a.connect(peer_b.clone()).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    handle_b.accept(session_id).await.unwrap();
    wait_for_connected(&mut events_a, session_id).await;
    wait_for_connected(&mut events_b, session_id).await;

    handle_a.ensure_trust_suspended(peer_b.clone(), barrier_owner).await.unwrap();
    let closed = wait_for_event(&mut events_a, |event| {
        matches!(
            event,
            Event::Closed {
                session_id: id,
                reason: CloseReason::TrustRevoked,
                ..
            } if *id == session_id
        )
    })
    .await;
    assert!(matches!(closed, Event::Closed { reason: CloseReason::TrustRevoked, .. }));
    assert!(handle_a.snapshot().session_for_peer(&peer_b).is_none());
    assert_eq!(handle_a.connect(peer_b.clone()).await, Err(Error::PeerSuspended(peer_b.clone())));
    wait_for_closed(&mut events_b, session_id).await;
    wait_for_absent(&handle_b, session_id).await;

    handle_a.ensure_trust_suspended(peer_b.clone(), barrier_owner).await.unwrap();
    handle_a.release_trust_suspension(peer_b.clone(), barrier_owner).await.unwrap();
    handle_a.release_trust_suspension(peer_b.clone(), barrier_owner).await.unwrap();
    let reconnected = handle_a.connect(peer_b).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, reconnected);
    handle_b.accept(reconnected).await.unwrap();
    wait_for_connected(&mut events_a, reconnected).await;
    wait_for_connected(&mut events_b, reconnected).await;

    tokio::time::timeout(Duration::from_secs(6), async {
        let (result_a, result_b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        result_a.unwrap();
        result_b.unwrap();
    })
    .await
    .expect("resumed peer services did not shut down within their owner deadline");
}

#[tokio::test]
async fn independent_trust_barrier_owners_release_independently() {
    let directory = Arc::new(TestDirectory::default());
    let mut config = Config::new(directory.clone(), directory);
    config.local_peer_id = Some(PeerId::new("local").unwrap());
    let mut service = PeerSessionService::start(config).await.unwrap();
    let handle = service.service_handle();
    let remote_peer = PeerId::new("remote").unwrap();
    let owner_a = TrustBarrierOwnerId::allocate();
    let owner_b = TrustBarrierOwnerId::allocate();

    handle.ensure_trust_suspended(remote_peer.clone(), owner_a).await.unwrap();
    handle.ensure_trust_suspended(remote_peer.clone(), owner_b).await.unwrap();
    handle.release_trust_suspension(remote_peer.clone(), owner_a).await.unwrap();
    assert_eq!(
        handle.connect(remote_peer.clone()).await,
        Err(Error::PeerSuspended(remote_peer.clone()))
    );

    handle.release_trust_suspension(remote_peer.clone(), owner_b).await.unwrap();
    assert_eq!(handle.connect(remote_peer.clone()).await, Err(Error::PeerNotTrusted(remote_peer)));
    service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admitted_commands_queued_behind_connect_get_service_stopped_on_shutdown() {
    let directory = Arc::new(BlockingDirectory::default());
    let mut config = Config::new(directory.clone(), directory.clone());
    config.local_peer_id = Some(PeerId::new("local").unwrap());
    let mut service = PeerSessionService::start(config).await.unwrap();
    let handle = service.service_handle();
    let first_handle = handle.clone();
    let first =
        tokio::spawn(async move { first_handle.connect(PeerId::new("first").unwrap()).await });
    directory.entered.notified().await;

    let (queued_reply_tx, queued_reply_rx) = oneshot::channel();
    handle
        .command_sender()
        .send(Command::Connect { peer_id: PeerId::new("queued").unwrap(), reply: queued_reply_tx })
        .await
        .unwrap();

    service.shutdown(ShutdownContext::default()).await.unwrap();
    assert_eq!(first.await.unwrap(), Err(Error::ServiceStopped));
    assert_eq!(queued_reply_rx.await.unwrap(), Err(Error::ServiceStopped));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mandatory_event_sink_overflow_fails_closed_and_blocks_new_sessions() {
    let peer_a = PeerId::new("peer-a").unwrap();
    let peer_b = PeerId::new("peer-b").unwrap();
    let directory_a = Arc::new(TestDirectory::default());
    let directory_b = Arc::new(TestDirectory::default());
    let mut config_a = Config::new(directory_a.clone(), directory_a.clone());
    config_a.local_peer_id = Some(peer_a.clone());
    let mut config_b = Config::new(directory_b.clone(), directory_b.clone());
    config_b.local_peer_id = Some(peer_b.clone());
    let (sink_tx, _sink_rx) = mpsc::channel(1);
    config_b.mandatory_event_sink = Some(sink_tx);

    let mut service_a = PeerSessionService::start(config_a).await.unwrap();
    let mut service_b = PeerSessionService::start(config_b).await.unwrap();
    directory_a.insert_peer(
        peer_b.clone(),
        service_b.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
    );
    directory_b.insert_peer(
        peer_a.clone(),
        service_a.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
    );
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();

    let session_id = handle_a.connect(peer_b.clone()).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    handle_b.accept(session_id).await.unwrap();
    wait_for_closed(&mut events_a, session_id).await;
    wait_for_closed(&mut events_b, session_id).await;
    wait_for_absent(&handle_a, session_id).await;
    wait_for_absent(&handle_b, session_id).await;

    let retry_id = handle_a.connect(peer_b).await.unwrap();
    wait_for_closed(&mut events_a, retry_id).await;
    assert!(handle_b.snapshot().sessions.is_empty());

    tokio::time::timeout(Duration::from_secs(6), async {
        let (result_a, result_b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        result_a.unwrap();
        result_b.unwrap();
    })
    .await
    .expect("services did not shut down after mandatory sink failure");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mandatory_event_sink_closure_proactively_terminates_an_idle_session() {
    let peer_a = PeerId::new("peer-a").unwrap();
    let peer_b = PeerId::new("peer-b").unwrap();
    let directory_a = Arc::new(TestDirectory::default());
    let directory_b = Arc::new(TestDirectory::default());
    let mut config_a = Config::new(directory_a.clone(), directory_a.clone());
    config_a.local_peer_id = Some(peer_a.clone());
    let mut config_b = Config::new(directory_b.clone(), directory_b.clone());
    config_b.local_peer_id = Some(peer_b.clone());
    let (sink_tx, sink_rx) = mpsc::channel(16);
    config_b.mandatory_event_sink = Some(sink_tx);

    let mut service_a = PeerSessionService::start(config_a).await.unwrap();
    let mut service_b = PeerSessionService::start(config_b).await.unwrap();
    directory_a.insert_peer(
        peer_b.clone(),
        service_b.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
    );
    directory_b.insert_peer(
        peer_a.clone(),
        service_a.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
    );
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();
    let session_id = handle_a.connect(peer_b).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    handle_b.accept(session_id).await.unwrap();
    wait_for_connected(&mut events_a, session_id).await;
    wait_for_connected(&mut events_b, session_id).await;

    drop(sink_rx);
    wait_for_closed(&mut events_a, session_id).await;
    wait_for_closed(&mut events_b, session_id).await;
    wait_for_absent(&handle_a, session_id).await;
    wait_for_absent(&handle_b, session_id).await;

    tokio::time::timeout(Duration::from_secs(6), async {
        let (result_a, result_b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        result_a.unwrap();
        result_b.unwrap();
    })
    .await
    .expect("services did not shut down after mandatory sink closure");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_peer_loopback_covers_reject_message_receipt_share_and_reconnect() {
    let peer_a = PeerId::new("peer-a").unwrap();
    let peer_b = PeerId::new("peer-b").unwrap();
    let directory_a = Arc::new(TestDirectory::default());
    let directory_b = Arc::new(TestDirectory::default());
    let mut config_a = Config::new(directory_a.clone(), directory_a.clone());
    config_a.local_peer_id = Some(peer_a.clone());
    config_a.limits.request_timeout = Duration::from_secs(5);
    config_a.limits.negotiation_timeout = Duration::from_secs(10);
    let mut config_b = Config::new(directory_b.clone(), directory_b.clone());
    config_b.local_peer_id = Some(peer_b.clone());
    config_b.limits.request_timeout = Duration::from_secs(5);
    config_b.limits.negotiation_timeout = Duration::from_secs(10);

    let mut service_a = PeerSessionService::start(config_a).await.unwrap();
    let mut service_b = PeerSessionService::start(config_b).await.unwrap();
    directory_a.insert_peer(
        peer_b.clone(),
        service_b.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
    );
    directory_b.insert_peer(
        peer_a.clone(),
        service_a.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
    );
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();

    // First prove a rejected request is terminal and leaves both registries reusable.
    let rejected_id = handle_a.connect(peer_b.clone()).await.unwrap();
    let incoming_id = wait_for_incoming(&mut events_b, &peer_a).await;
    assert_eq!(incoming_id, rejected_id);
    handle_b.reject(incoming_id, "not now").await.unwrap();
    wait_for_closed(&mut events_a, rejected_id).await;
    wait_for_closed(&mut events_b, incoming_id).await;
    wait_for_absent(&handle_a, rejected_id).await;
    wait_for_absent(&handle_b, incoming_id).await;

    // Reconnect on the same long-lived services, accept, and exercise session capabilities.
    let session_id = handle_a.connect(peer_b.clone()).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    handle_b.accept(session_id).await.unwrap();
    wait_for_connected(&mut events_a, session_id).await;
    wait_for_connected(&mut events_b, session_id).await;

    let message_id = MessageId::new();
    let sent_at = Utc::now();
    handle_a.send_message(session_id, message_id, "hello", sent_at).await.unwrap();
    let received = wait_for_event(&mut events_b, |event| {
        matches!(
            event,
            Event::MessageReceived {
                session_id: id,
                message_id: received_id,
                body,
                ..
            } if *id == session_id && *received_id == message_id && body == "hello"
        )
    })
    .await;
    assert!(matches!(received, Event::MessageReceived { .. }));

    let received_at = Utc::now();
    handle_b.send_receipt(session_id, message_id, received_at).await.unwrap();
    wait_for_event(&mut events_a, |event| {
        matches!(
            event,
            Event::MessageReceiptReceived {
                session_id: id,
                message_id: received_id,
                ..
            } if *id == session_id && *received_id == message_id
        )
    })
    .await;

    let share_id = handle_a.start_screen_share(session_id).await.unwrap();
    wait_for_event(&mut events_b, |event| {
        matches!(
            event,
            Event::RemoteShareChanged {
                session_id: id,
                state: super::super::RemoteShareState::Active { share_id: remote_id, .. },
                ..
            } if *id == session_id && *remote_id == share_id
        )
    })
    .await;
    handle_a.stop_screen_share(session_id, share_id).await.unwrap();
    wait_for_event(&mut events_b, |event| {
        matches!(
            event,
            Event::RemoteShareChanged {
                session_id: id,
                state: super::super::RemoteShareState::Inactive,
                ..
            } if *id == session_id
        )
    })
    .await;

    handle_a.disconnect(session_id).await.unwrap();
    wait_for_closed(&mut events_a, session_id).await;
    wait_for_closed(&mut events_b, session_id).await;
    wait_for_absent(&handle_a, session_id).await;
    wait_for_absent(&handle_b, session_id).await;

    tokio::time::timeout(Duration::from_secs(5), service_a.shutdown(ShutdownContext::default()))
        .await
        .expect("service A shutdown timed out")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), service_b.shutdown(ShutdownContext::default()))
        .await
        .expect("service B shutdown timed out")
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rtp_share_epoch_survives_fragmentation_and_rejects_a_stale_sink() {
    fn h264_sample(marker: u8, size: usize) -> super::super::EncodedVideoSample {
        let mut data = vec![marker; size.max(3)];
        data[0] = 0x65;
        super::super::EncodedVideoSample::new(data, Duration::from_millis(16))
    }

    async fn receive_marker(source: &mut RemoteVideoSource, epoch: super::super::ShareEpoch) -> u8 {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match source.recv_for(epoch).await.unwrap() {
                    super::super::RemoteVideoRead::Sample(sample) => {
                        if let Some(marker) = sample.data.get(5) {
                            return *marker;
                        }
                    }
                    super::super::RemoteVideoRead::EpochAdvanced { next_epoch } => {
                        panic!(
                            "media advanced to epoch {} while waiting for {}",
                            next_epoch.value(),
                            epoch.value()
                        );
                    }
                }
            }
        })
        .await
        .expect("timed out waiting for epoch-tagged video")
    }

    let (mut service_a, mut service_b, peer_a, peer_b) = start_test_pair().await;
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();

    let session_id = handle_a.connect(peer_b).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    handle_b.accept(session_id).await.unwrap();
    wait_for_connected(&mut events_a, session_id).await;
    wait_for_connected(&mut events_b, session_id).await;
    let mut remote = handle_b.subscribe_remote_video(session_id).await.unwrap();

    let share_a = handle_a.start_screen_share(session_id).await.unwrap();
    wait_for_event(&mut events_b, |event| {
        matches!(
            event,
            Event::RemoteShareChanged {
                session_id: id,
                state: super::super::RemoteShareState::Active {
                    share_id,
                    epoch: super::super::ShareEpoch::FIRST,
                },
                ..
            } if *id == session_id && *share_id == share_a
        )
    })
    .await;
    let sink_a = handle_a.encoded_video_sink(session_id, share_a).await.unwrap();
    sink_a.send(h264_sample(0xa1, 5_000)).await.unwrap();
    sink_a.send(h264_sample(0xa2, 32)).await.unwrap();
    sink_a.send(h264_sample(0xa3, 32)).await.unwrap();
    assert_eq!(receive_marker(&mut remote, super::super::ShareEpoch::FIRST).await, 0xa1);

    handle_a.stop_screen_share(session_id, share_a).await.unwrap();
    wait_for_event(&mut events_b, |event| {
        matches!(
            event,
            Event::RemoteShareChanged {
                session_id: id,
                state: super::super::RemoteShareState::Inactive,
                ..
            } if *id == session_id
        )
    })
    .await;
    let share_b = handle_a.start_screen_share(session_id).await.unwrap();
    let epoch_b = super::super::ShareEpoch::FIRST.next().unwrap();
    wait_for_event(&mut events_b, |event| {
        matches!(
            event,
            Event::RemoteShareChanged {
                session_id: id,
                state: super::super::RemoteShareState::Active { share_id, epoch },
                ..
            } if *id == session_id && *share_id == share_b && *epoch == epoch_b
        )
    })
    .await;

    // The old producer can still race cancellation, but its immutable A
    // capability is revoked and cannot backpressure or be relabelled as B.
    assert_eq!(sink_a.send(h264_sample(0xaf, 5_000)).await, Err(Error::MediaClosed));
    let sink_b = handle_a.encoded_video_sink(session_id, share_b).await.unwrap();
    sink_b.send(h264_sample(0xb1, 5_000)).await.unwrap();
    sink_b.send(h264_sample(0xb2, 32)).await.unwrap();
    sink_b.send(h264_sample(0xb3, 32)).await.unwrap();
    assert_eq!(receive_marker(&mut remote, epoch_b).await, 0xb1);

    handle_a.disconnect(session_id).await.unwrap();
    wait_for_closed(&mut events_a, session_id).await;
    wait_for_closed(&mut events_b, session_id).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        let (a, b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        a.unwrap();
        b.unwrap();
    })
    .await
    .expect("media epoch test services did not shut down");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ice_restart_preserves_session_channels_and_active_share_state() {
    let (mut service_a, mut service_b, peer_a, peer_b) = start_test_pair().await;
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();

    let session_id = handle_a.connect(peer_b).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    handle_b.accept(session_id).await.unwrap();
    wait_for_connected(&mut events_a, session_id).await;
    wait_for_connected(&mut events_b, session_id).await;

    let share_id = handle_a.start_screen_share(session_id).await.unwrap();
    wait_for_event(&mut events_b, |event| {
        matches!(
            event,
            Event::RemoteShareChanged {
                session_id: id,
                state: super::super::RemoteShareState::Active { share_id: remote_id, .. },
                ..
            } if *id == session_id && *remote_id == share_id
        )
    })
    .await;

    // Let the initial DTLS/SCTP association leave its just-open callback
    // window before forcing a healthy-path restart in this test. Production
    // recovery is triggered only after the configured ICE disconnect grace.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let generation_a = handle_a.committed_transport_generation(session_id).await.unwrap();
    let generation_b = handle_b.committed_transport_generation(session_id).await.unwrap();
    handle_a.force_ice_restart(session_id).await.unwrap();
    let ((), ()) = tokio::join!(
        wait_for_transport_generation(&handle_a, session_id, generation_a),
        wait_for_transport_generation(&handle_b, session_id, generation_b),
    );

    let snapshot_a = handle_a.snapshot();
    let snapshot_b = handle_b.snapshot();
    let session_a = snapshot_a.session(session_id).expect("offerer session survived restart");
    let session_b = snapshot_b.session(session_id).expect("answerer session survived restart");
    assert_eq!(
        session_a.local_share,
        super::super::LocalShareState::Active { share_id, epoch: super::super::ShareEpoch::FIRST }
    );
    assert_eq!(
        session_b.remote_share,
        super::super::RemoteShareState::Active { share_id, epoch: super::super::ShareEpoch::FIRST }
    );
    assert_eq!(snapshot_a.sessions.len(), 1);
    assert_eq!(snapshot_b.sessions.len(), 1);

    let message_id = MessageId::new();
    handle_a.send_message(session_id, message_id, "after restart", Utc::now()).await.unwrap();
    let post_restart_event = wait_for_event(&mut events_b, |event| {
        matches!(
            event,
            Event::MessageReceived {
                session_id: id,
                message_id: received_id,
                body,
                ..
            } if *id == session_id && *received_id == message_id && body == "after restart"
        ) || matches!(
            event,
            Event::Closed { session_id: id, .. } if *id == session_id
        )
    })
    .await;
    assert!(
        matches!(post_restart_event, Event::MessageReceived { .. }),
        "session closed after restart instead of delivering data: {post_restart_event:?}"
    );

    handle_a.disconnect(session_id).await.unwrap();
    wait_for_closed(&mut events_a, session_id).await;
    wait_for_closed(&mut events_b, session_id).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        let (a, b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        a.unwrap();
        b.unwrap();
    })
    .await
    .expect("restart test services did not shut down");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn answerer_initiated_and_simultaneous_restarts_converge_without_glare() {
    let (mut service_a, mut service_b, peer_a, peer_b) = start_test_pair().await;
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();
    let session_id = handle_a.connect(peer_b).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    handle_b.accept(session_id).await.unwrap();
    wait_for_connected(&mut events_a, session_id).await;
    wait_for_connected(&mut events_b, session_id).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let generation_a = handle_a.committed_transport_generation(session_id).await.unwrap();
    let generation_b = handle_b.committed_transport_generation(session_id).await.unwrap();
    let (forced, (), ()) = tokio::join!(
        handle_b.force_ice_restart(session_id),
        wait_for_transport_generation(&handle_a, session_id, generation_a),
        wait_for_transport_generation(&handle_b, session_id, generation_b),
    );
    forced.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;
    let generation_a = handle_a.committed_transport_generation(session_id).await.unwrap();
    let generation_b = handle_b.committed_transport_generation(session_id).await.unwrap();
    let (forced_a, forced_b, (), ()) = tokio::join!(
        handle_a.force_ice_restart(session_id),
        handle_b.force_ice_restart(session_id),
        wait_for_transport_generation(&handle_a, session_id, generation_a),
        wait_for_transport_generation(&handle_b, session_id, generation_b),
    );
    forced_a.unwrap();
    forced_b.unwrap();

    assert_eq!(handle_a.snapshot().sessions.len(), 1);
    assert_eq!(handle_b.snapshot().sessions.len(), 1);
    let message_id = MessageId::new();
    handle_b
        .send_message(session_id, message_id, "after simultaneous restart", Utc::now())
        .await
        .unwrap();
    wait_for_event(&mut events_a, |event| {
        matches!(
            event,
            Event::MessageReceived {
                session_id: id,
                message_id: received_id,
                body,
                ..
            } if *id == session_id
                && *received_id == message_id
                && body == "after simultaneous restart"
        )
    })
    .await;

    handle_a.disconnect(session_id).await.unwrap();
    wait_for_closed(&mut events_a, session_id).await;
    wait_for_closed(&mut events_b, session_id).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        let (a, b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        a.unwrap();
        b.unwrap();
    })
    .await
    .expect("simultaneous restart services did not shut down");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unreachable_restart_signaling_is_removed_within_the_attempt_deadline() {
    let peer_a = PeerId::new("peer-a").unwrap();
    let peer_b = PeerId::new("peer-b").unwrap();
    let directory_a = Arc::new(TestDirectory::default());
    let directory_b = Arc::new(TestDirectory::default());
    let mut config_a = Config::new(directory_a.clone(), directory_a.clone());
    config_a.local_peer_id = Some(peer_a.clone());
    config_a.limits.ice_restart_timeout = Duration::from_millis(150);
    config_a.limits.endpoint_attempt_timeout = Duration::from_secs(5);
    config_a.limits.shutdown_timeout = Duration::from_millis(500);
    let mut config_b = Config::new(directory_b.clone(), directory_b.clone());
    config_b.local_peer_id = Some(peer_b.clone());
    config_b.limits.shutdown_timeout = Duration::from_millis(500);
    let mut service_a = PeerSessionService::start(config_a).await.unwrap();
    let mut service_b = PeerSessionService::start(config_b).await.unwrap();
    directory_a.insert_peer(
        peer_b.clone(),
        service_b.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_b.signaling_port())),
    );
    directory_b.insert_peer(
        peer_a.clone(),
        service_a.local_public_key(),
        SocketAddr::from(([127, 0, 0, 1], service_a.signaling_port())),
    );
    let handle_a = service_a.service_handle();
    let handle_b = service_b.service_handle();
    let mut events_a = handle_a.events();
    let mut events_b = handle_b.events();
    let session_id = handle_a.connect(peer_b.clone()).await.unwrap();
    assert_eq!(wait_for_incoming(&mut events_b, &peer_a).await, session_id);
    handle_b.accept(session_id).await.unwrap();
    wait_for_connected(&mut events_a, session_id).await;
    wait_for_connected(&mut events_b, session_id).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    directory_a.insert_peer_with_hints(
        peer_b,
        service_b.local_public_key(),
        Arc::from([SocketAddr::from(([203, 0, 113, 1], 9))]),
    );
    let started = Instant::now();
    handle_a.force_ice_restart(session_id).await.unwrap();
    wait_for_closed(&mut events_a, session_id).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "restart removal exceeded its absolute deadline: {:?}",
        started.elapsed()
    );
    wait_for_absent(&handle_a, session_id).await;

    tokio::time::timeout(Duration::from_secs(5), async {
        let (a, b) = tokio::join!(
            service_a.shutdown(ShutdownContext::default()),
            service_b.shutdown(ShutdownContext::default()),
        );
        a.unwrap();
        b.unwrap();
    })
    .await
    .expect("timed-out restart services did not shut down");
}

async fn wait_for_incoming(events: &mut broadcast::Receiver<Event>, peer_id: &PeerId) -> SessionId {
    match wait_for_event(
        events,
        |event| matches!(event, Event::IncomingRequest { peer_id: id, .. } if id == peer_id),
    )
    .await
    {
        Event::IncomingRequest { session_id, .. } => session_id,
        _ => unreachable!(),
    }
}

async fn wait_for_connected(events: &mut broadcast::Receiver<Event>, session_id: SessionId) {
    wait_for_event(
        events,
        |event| matches!(event, Event::Connected { session_id: id, .. } if *id == session_id),
    )
    .await;
}

async fn wait_for_closed(events: &mut broadcast::Receiver<Event>, session_id: SessionId) {
    wait_for_event(
        events,
        |event| matches!(event, Event::Closed { session_id: id, .. } if *id == session_id),
    )
    .await;
}

async fn wait_for_event(
    events: &mut broadcast::Receiver<Event>,
    predicate: impl Fn(&Event) -> bool,
) -> Event {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let event = events.recv().await.expect("session event channel closed");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for peer-session event")
}

async fn wait_for_absent(handle: &ServiceHandle, session_id: SessionId) {
    let mut snapshots = handle.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        while snapshots.borrow().session(session_id).is_some() {
            snapshots.changed().await.expect("session snapshot channel closed");
        }
    })
    .await
    .expect("session was not removed");
}

async fn wait_for_transport_generation(
    handle: &ServiceHandle,
    session_id: SessionId,
    previous: u64,
) {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let current = handle
                .committed_transport_generation(session_id)
                .await
                .expect("session disappeared before committing its restart");
            if current > previous {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("session did not commit its next transport generation");
}

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::{Notify, mpsc, oneshot};

use super::{actor::Command, transport::SessionMessaging, *};
use crate::{
    identity::PeerId,
    peer_session::{self, LocalShareState, MessageId, RemoteShareState, SessionId, SessionState},
    service_host::{HostedService, ShutdownContext},
};

#[derive(Default)]
struct FakeStoreState {
    next_id: i64,
    messages: Vec<MessageRecord>,
}

#[derive(Default)]
struct FakeMessagesStore {
    state: Mutex<FakeStoreState>,
    block_list: AtomicBool,
    list_started: Notify,
    release_list: Notify,
}

impl FakeMessagesStore {
    fn models(&self) -> Vec<MessageRecord> {
        self.state.lock().unwrap().messages.clone()
    }

    fn insert_model(&self, model: MessageRecord) {
        let mut state = self.state.lock().unwrap();
        state.next_id = state.next_id.max(model.id);
        state.messages.push(model);
    }

    fn block_list(&self) {
        self.block_list.store(true, Ordering::SeqCst);
    }

    async fn wait_for_blocked_list(&self) {
        self.list_started.notified().await;
    }

    fn release_list(&self) {
        self.block_list.store(false, Ordering::SeqCst);
        self.release_list.notify_one();
    }
}

#[async_trait]
impl Store for FakeMessagesStore {
    async fn list(&self) -> Result<Vec<MessageRecord>, StoreError> {
        if self.block_list.load(Ordering::SeqCst) {
            self.list_started.notify_one();
            self.release_list.notified().await;
        }
        Ok(self.models())
    }

    async fn create_outgoing(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        peer_id: PeerId,
        body: String,
        created_at: DateTime<Utc>,
    ) -> Result<MessageRecord, StoreError> {
        let mut state = self.state.lock().unwrap();
        state.next_id += 1;
        let model = MessageRecord {
            id: state.next_id,
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            peer_id: peer_id.to_string(),
            direction: "outgoing".into(),
            body,
            status: "pending".into(),
            created_at,
            delivered_at: None,
        };
        state.messages.push(model.clone());
        Ok(model)
    }

    async fn create_incoming_if_missing(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        peer_id: PeerId,
        body: String,
        created_at: DateTime<Utc>,
        received_at: DateTime<Utc>,
    ) -> Result<Option<MessageRecord>, StoreError> {
        let mut state = self.state.lock().unwrap();
        if state.messages.iter().any(|message| {
            message.peer_id == peer_id.as_str()
                && message.message_id == message_id.to_string()
                && message.direction == "incoming"
        }) {
            return Ok(None);
        }
        state.next_id += 1;
        let model = MessageRecord {
            id: state.next_id,
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            peer_id: peer_id.to_string(),
            direction: "incoming".into(),
            body,
            status: "delivered".into(),
            created_at,
            delivered_at: Some(received_at),
        };
        state.messages.push(model.clone());
        Ok(Some(model))
    }

    async fn mark_sent(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageRecord>, StoreError> {
        Ok(transition(
            &mut self.state.lock().unwrap().messages,
            session_id,
            &peer_id,
            message_id,
            "pending",
            "sent",
            None,
        ))
    }

    async fn mark_delivered(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
        delivered_at: DateTime<Utc>,
    ) -> Result<Option<MessageRecord>, StoreError> {
        let mut state = self.state.lock().unwrap();
        let Some(message) = state.messages.iter_mut().find(|message| {
            message.session_id == session_id.to_string()
                && message.peer_id == peer_id.as_str()
                && message.message_id == message_id.to_string()
                && message.direction == "outgoing"
                && matches!(message.status.as_str(), "pending" | "sent" | "unknown")
        }) else {
            return Ok(None);
        };
        message.status = "delivered".into();
        message.delivered_at = Some(delivered_at);
        Ok(Some(message.clone()))
    }

    async fn mark_failed(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageRecord>, StoreError> {
        Ok(transition(
            &mut self.state.lock().unwrap().messages,
            session_id,
            &peer_id,
            message_id,
            "pending",
            "failed",
            None,
        ))
    }

    async fn mark_unknown(
        &self,
        session_id: SessionId,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> Result<Option<MessageRecord>, StoreError> {
        Ok(transition(
            &mut self.state.lock().unwrap().messages,
            session_id,
            &peer_id,
            message_id,
            "pending",
            "unknown",
            None,
        ))
    }

    async fn mark_all_pending_unknown(&self) -> Result<Vec<MessageRecord>, StoreError> {
        let mut state = self.state.lock().unwrap();
        let mut changed = Vec::new();
        for message in &mut state.messages {
            if message.direction == "outgoing" && message.status == "pending" {
                message.status = "unknown".into();
                changed.push(message.clone());
            }
        }
        Ok(changed)
    }
}

fn transition(
    messages: &mut [MessageRecord],
    session_id: SessionId,
    peer_id: &PeerId,
    message_id: MessageId,
    from: &str,
    to: &str,
    delivered_at: Option<DateTime<Utc>>,
) -> Option<MessageRecord> {
    let message = messages.iter_mut().find(|message| {
        message.session_id == session_id.to_string()
            && message.peer_id == peer_id.as_str()
            && message.message_id == message_id.to_string()
            && message.direction == "outgoing"
            && message.status == from
    })?;
    message.status = to.into();
    message.delivered_at = delivered_at;
    Some(message.clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SentMessage {
    session_id: SessionId,
    message_id: MessageId,
    body: String,
    sent_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SentReceipt {
    session_id: SessionId,
    message_id: MessageId,
    received_at: DateTime<Utc>,
}

struct FakeSessions {
    snapshot: peer_session::Sessions,
    fail_send: AtomicBool,
    unknown_send: AtomicBool,
    sent: Mutex<Vec<SentMessage>>,
    receipt_tx: mpsc::UnboundedSender<SentReceipt>,
}

#[async_trait]
impl SessionMessaging for FakeSessions {
    fn snapshot(&self) -> peer_session::Sessions {
        self.snapshot.clone()
    }

    async fn send_message(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        body: String,
        sent_at: DateTime<Utc>,
    ) -> Result<(), peer_session::Error> {
        if self.unknown_send.load(Ordering::SeqCst) {
            return Err(peer_session::Error::OutcomeUnknown);
        }
        if self.fail_send.load(Ordering::SeqCst) {
            return Err(peer_session::Error::ServiceStopped);
        }
        self.sent.lock().unwrap().push(SentMessage { session_id, message_id, body, sent_at });
        Ok(())
    }

    async fn send_receipt(
        &self,
        session_id: SessionId,
        message_id: MessageId,
        received_at: DateTime<Utc>,
    ) -> Result<(), peer_session::Error> {
        self.receipt_tx
            .send(SentReceipt { session_id, message_id, received_at })
            .map_err(|_| peer_session::Error::ServiceStopped)
    }
}

struct Harness {
    service: MessagingService,
    handle: ServiceHandle,
    store: Arc<FakeMessagesStore>,
    sessions: Arc<FakeSessions>,
    session_event_tx: mpsc::Sender<peer_session::Event>,
    receipt_rx: mpsc::UnboundedReceiver<SentReceipt>,
    session_id: SessionId,
    peer_id: PeerId,
}

async fn harness(connected: bool) -> Harness {
    harness_with_limits(connected, Limits::default()).await
}

async fn harness_with_limits(connected: bool, limits: Limits) -> Harness {
    let store = Arc::new(FakeMessagesStore::default());
    let session_id = SessionId::new();
    let peer_id = PeerId::new("peer-a").unwrap();
    let snapshot = peer_session::Sessions {
        sessions: Arc::new(vec![SessionState {
            session_id,
            peer_id: peer_id.clone(),
            phase: if connected {
                peer_session::Phase::Connected
            } else {
                peer_session::Phase::Negotiating
            },
            local_share: LocalShareState::Inactive,
            remote_share: RemoteShareState::Inactive,
        }]),
    };
    let (receipt_tx, receipt_rx) = mpsc::unbounded_channel();
    let sessions = Arc::new(FakeSessions {
        snapshot,
        fail_send: AtomicBool::new(false),
        unknown_send: AtomicBool::new(false),
        sent: Mutex::new(Vec::new()),
        receipt_tx,
    });
    let (session_event_tx, session_event_rx) = mpsc::channel(16);
    let service = MessagingService::start_with_transport_and_limits(
        store.clone(),
        sessions.clone(),
        session_event_rx,
        limits,
    )
    .await
    .unwrap();
    let handle = service.service_handle();

    Harness { service, handle, store, sessions, session_event_tx, receipt_rx, session_id, peer_id }
}

#[tokio::test]
async fn outgoing_message_moves_from_pending_to_sent_without_retry() {
    let mut harness = harness(true).await;
    let mut events = harness.handle.events();

    let message_id = harness
        .handle
        .send_message(harness.session_id, harness.peer_id.clone(), " hello ".into())
        .await
        .unwrap();

    let messages = harness.handle.snapshot().messages_for_peer(&harness.peer_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message_id, message_id);
    assert_eq!(messages[0].body, "hello");
    assert_eq!(messages[0].status, MessageStatus::Sent);
    assert_eq!(harness.sessions.sent.lock().unwrap().len(), 1);
    assert!(matches!(events.recv().await.unwrap(), Event::ConversationUpdated { .. }));

    harness.service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn failed_session_send_is_persisted_as_failed() {
    let mut harness = harness(true).await;
    harness.sessions.fail_send.store(true, Ordering::SeqCst);

    assert!(matches!(
        harness
            .handle
            .send_message(harness.session_id, harness.peer_id.clone(), "hello".into())
            .await,
        Err(Error::Session(peer_session::Error::ServiceStopped))
    ));
    let messages = harness.handle.snapshot().messages_for_peer(&harness.peer_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].status, MessageStatus::Failed);
    assert!(harness.sessions.sent.lock().unwrap().is_empty());

    harness.service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn ambiguous_transport_send_is_persisted_as_delivery_unknown() {
    let mut harness = harness(true).await;
    harness.sessions.unknown_send.store(true, Ordering::SeqCst);

    assert!(matches!(
        harness
            .handle
            .send_message(harness.session_id, harness.peer_id.clone(), "hello".into())
            .await,
        Err(Error::Session(peer_session::Error::OutcomeUnknown))
    ));
    let messages = harness.handle.snapshot().messages_for_peer(&harness.peer_id);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].status, MessageStatus::Unknown);

    harness.service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn authenticated_receipt_marks_the_matching_outgoing_message_delivered() {
    let mut harness = harness(true).await;
    let message_id = harness
        .handle
        .send_message(harness.session_id, harness.peer_id.clone(), "hello".into())
        .await
        .unwrap();
    let mut snapshots = harness.handle.subscribe();
    // A cloned watch receiver can inherit an unseen pre-subscription version.
    // Mark the current `Sent` snapshot as observed before waiting for the receipt update.
    snapshots.borrow_and_update();
    let received_at = Utc::now();

    harness
        .session_event_tx
        .send(peer_session::Event::MessageReceiptReceived {
            session_id: harness.session_id,
            peer_id: harness.peer_id.clone(),
            message_id,
            received_at,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), snapshots.changed()).await.unwrap().unwrap();

    let messages = harness.handle.snapshot().messages_for_peer(&harness.peer_id);
    assert_eq!(messages[0].status, MessageStatus::Delivered);
    assert_eq!(messages[0].delivered_at, Some(received_at));
    harness.service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn send_requires_the_exact_connected_session_and_peer() {
    let mut connected_harness = harness(true).await;
    let other_peer = PeerId::new("peer-b").unwrap();

    assert!(matches!(
        connected_harness
            .handle
            .send_message(connected_harness.session_id, other_peer, "hello".into())
            .await,
        Err(Error::SessionPeerMismatch { .. })
    ));
    assert!(matches!(
        connected_harness
            .handle
            .send_message(SessionId::new(), connected_harness.peer_id.clone(), "hello".into(),)
            .await,
        Err(Error::SessionNotConnected { .. })
    ));
    assert!(connected_harness.store.models().is_empty());

    connected_harness.service.shutdown(ShutdownContext::default()).await.unwrap();

    let mut negotiating_harness = harness(false).await;
    assert!(matches!(
        negotiating_harness
            .handle
            .send_message(
                negotiating_harness.session_id,
                negotiating_harness.peer_id.clone(),
                "hello".into(),
            )
            .await,
        Err(Error::SessionNotConnected { .. })
    ));
    negotiating_harness.service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn duplicate_incoming_message_is_stored_once_and_acknowledged_each_time() {
    let mut harness = harness(true).await;
    let message_id = MessageId::new();
    let sent_at = Utc::now();
    let event = peer_session::Event::MessageReceived {
        session_id: harness.session_id,
        peer_id: harness.peer_id.clone(),
        message_id,
        body: "hello".into(),
        sent_at,
    };
    let mut snapshots = harness.handle.subscribe();

    harness.session_event_tx.send(event.clone()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), snapshots.changed()).await.unwrap().unwrap();
    harness.session_event_tx.send(event).await.unwrap();

    let first_receipt = tokio::time::timeout(Duration::from_secs(1), harness.receipt_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let second_receipt = tokio::time::timeout(Duration::from_secs(1), harness.receipt_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first_receipt.message_id, message_id);
    assert_eq!(second_receipt.message_id, message_id);
    assert_eq!(harness.handle.snapshot().messages_for_peer(&harness.peer_id).len(), 1);
    assert_eq!(harness.store.models().len(), 1);

    harness.service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn receipt_from_wrong_peer_or_session_cannot_mark_message_delivered() {
    let mut harness = harness(true).await;
    let message_id = harness
        .handle
        .send_message(harness.session_id, harness.peer_id.clone(), "hello".into())
        .await
        .unwrap();
    let wrong_peer = PeerId::new("peer-b").unwrap();
    harness
        .session_event_tx
        .send(peer_session::Event::MessageReceiptReceived {
            session_id: harness.session_id,
            peer_id: wrong_peer.clone(),
            message_id,
            received_at: Utc::now(),
        })
        .await
        .unwrap();
    harness
        .session_event_tx
        .send(peer_session::Event::MessageReceiptReceived {
            session_id: SessionId::new(),
            peer_id: harness.peer_id.clone(),
            message_id,
            received_at: Utc::now(),
        })
        .await
        .unwrap();

    // A subsequent persisted event is a barrier proving the wrong receipt was processed first.
    harness
        .session_event_tx
        .send(peer_session::Event::MessageReceived {
            session_id: harness.session_id,
            peer_id: wrong_peer,
            message_id: MessageId::new(),
            body: "barrier".into(),
            sent_at: Utc::now(),
        })
        .await
        .unwrap();
    let mut receipt_rx = harness.receipt_rx;
    tokio::time::timeout(Duration::from_secs(1), receipt_rx.recv()).await.unwrap().unwrap();

    let outgoing = harness.handle.snapshot().messages_for_peer(&harness.peer_id);
    assert_eq!(outgoing[0].status, MessageStatus::Sent);

    harness.service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn startup_marks_pending_rows_unknown_instead_of_retrying_them() {
    let store = Arc::new(FakeMessagesStore::default());
    let session_id = SessionId::new();
    let message_id = MessageId::new();
    let peer_id = PeerId::new("peer-a").unwrap();
    store.insert_model(MessageRecord {
        id: 1,
        session_id: session_id.to_string(),
        message_id: message_id.to_string(),
        peer_id: peer_id.to_string(),
        direction: "outgoing".into(),
        body: "interrupted".into(),
        status: "pending".into(),
        created_at: Utc::now(),
        delivered_at: None,
    });
    let (receipt_tx, _) = mpsc::unbounded_channel();
    let sessions = Arc::new(FakeSessions {
        snapshot: peer_session::Sessions::default(),
        fail_send: AtomicBool::new(false),
        unknown_send: AtomicBool::new(false),
        sent: Mutex::new(Vec::new()),
        receipt_tx,
    });
    let (_event_tx, event_rx) = mpsc::channel(1);

    let mut service =
        MessagingService::start_with_transport(store, sessions, event_rx).await.unwrap();
    let messages = service.service_handle().snapshot().messages_for_peer(&peer_id);
    assert_eq!(messages[0].status, MessageStatus::Unknown);
    service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn shutdown_drains_an_already_accepted_incoming_message() {
    let mut harness = harness(true).await;
    let message_id = MessageId::new();
    harness
        .session_event_tx
        .send(peer_session::Event::MessageReceived {
            session_id: harness.session_id,
            peer_id: harness.peer_id.clone(),
            message_id,
            body: "persist before stopping".into(),
            sent_at: Utc::now(),
        })
        .await
        .unwrap();

    harness.service.shutdown(ShutdownContext::default()).await.unwrap();

    let stored = harness.store.models();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].message_id, message_id.to_string());
}

#[tokio::test]
async fn expired_queued_command_has_no_late_side_effect() {
    let limits = Limits {
        command_start_timeout: Duration::from_millis(25),
        shutdown_timeout: Duration::from_secs(1),
    };
    let mut harness = harness_with_limits(true, limits).await;
    harness.store.block_list();

    let refresh_handle = harness.handle.clone();
    let refresh = tokio::spawn(async move { refresh_handle.refresh().await });
    harness.store.wait_for_blocked_list().await;

    let send_handle = harness.handle.clone();
    let peer_id = harness.peer_id.clone();
    let session_id = harness.session_id;
    let send = tokio::spawn(async move {
        send_handle.send_message(session_id, peer_id, "must not send".into()).await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    harness.store.release_list();

    assert!(refresh.await.unwrap().is_ok());
    assert!(matches!(send.await.unwrap(), Err(Error::CommandExpired)));
    assert!(harness.store.models().is_empty());
    assert!(harness.sessions.sent.lock().unwrap().is_empty());
    harness.service.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn shutdown_rejects_queued_commands_before_they_start() {
    let harness = harness(true).await;
    harness.store.block_list();

    let refresh_handle = harness.handle.clone();
    let refresh = tokio::spawn(async move { refresh_handle.refresh().await });
    harness.store.wait_for_blocked_list().await;

    let (response_tx, response_rx) = oneshot::channel();
    harness
        .handle
        .enqueue(Command::SendMessage {
            deadline: Instant::now() + Duration::from_secs(1),
            session_id: harness.session_id,
            peer_id: harness.peer_id.clone(),
            body: "queued".into(),
            response_tx,
        })
        .unwrap();
    let mut service = harness.service;
    let shutdown = tokio::spawn(async move { service.shutdown(ShutdownContext::default()).await });
    tokio::task::yield_now().await;
    harness.store.release_list();

    assert!(refresh.await.unwrap().is_ok());
    assert!(matches!(response_rx.await.unwrap(), Err(Error::ServiceStopping)));
    assert!(shutdown.await.unwrap().is_ok());
    assert!(harness.store.models().is_empty());
}

#[tokio::test]
async fn saturated_command_admission_is_rejected_without_side_effects() {
    let harness = harness(true).await;
    harness.store.block_list();

    let refresh_handle = harness.handle.clone();
    let refresh = tokio::spawn(async move { refresh_handle.refresh().await });
    harness.store.wait_for_blocked_list().await;

    for _ in 0..COMMAND_CAPACITY {
        let (response_tx, _response_rx) = oneshot::channel();
        harness
            .handle
            .enqueue(Command::Refresh {
                deadline: Instant::now() + Duration::from_secs(1),
                response_tx,
            })
            .unwrap();
    }
    assert!(matches!(
        harness
            .handle
            .send_message(harness.session_id, harness.peer_id.clone(), "not admitted".into())
            .await,
        Err(Error::ServiceBusy)
    ));
    assert!(harness.store.models().is_empty());
    assert!(harness.sessions.sent.lock().unwrap().is_empty());

    let mut service = harness.service;
    let shutdown = tokio::spawn(async move { service.shutdown(ShutdownContext::default()).await });
    tokio::task::yield_now().await;
    harness.store.release_list();
    assert!(refresh.await.unwrap().is_ok());
    assert!(shutdown.await.unwrap().is_ok());
}

#[tokio::test]
async fn shutdown_aborts_and_detaches_a_stalled_actor_at_its_deadline() {
    let limits = Limits {
        command_start_timeout: Duration::from_secs(1),
        shutdown_timeout: Duration::from_millis(50),
    };
    let mut harness = harness_with_limits(true, limits).await;
    harness.store.block_list();

    let refresh_handle = harness.handle.clone();
    let refresh = tokio::spawn(async move { refresh_handle.refresh().await });
    harness.store.wait_for_blocked_list().await;

    let result = tokio::time::timeout(
        Duration::from_secs(1),
        harness.service.shutdown(ShutdownContext::default()),
    )
    .await
    .expect("owner shutdown must be bounded");
    assert!(matches!(result, Err(Error::ShutdownTimeout)));
    assert!(matches!(refresh.await.unwrap(), Err(Error::ResponseDropped)));
}

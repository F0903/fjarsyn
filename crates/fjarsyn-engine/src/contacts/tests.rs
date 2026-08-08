use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::Notify;

use super::{
    AdmissionControl, ContactRecord, ContactsService, Directory, Error, Store, StoreError,
};
use crate::{
    identity::{LocalPeerIdentity, PeerId},
    pairing::{Invite, VerifiedPeerIdentity},
    peer_session::{self, TrustBarrierOwnerId},
    service_host::{HostedService, ShutdownContext},
};

struct TestContactsStore {
    contacts: Mutex<Vec<ContactRecord>>,
    fail_update: AtomicBool,
    fail_update_after_commit: AtomicBool,
    fail_delete_after_commit: AtomicBool,
    fail_list: AtomicBool,
    block_update: AtomicBool,
    block_delete: AtomicBool,
    create_calls: AtomicUsize,
    update_calls: AtomicUsize,
    update_entered: Notify,
    release_update: Notify,
    delete_entered: Notify,
    release_delete: Notify,
}

impl TestContactsStore {
    fn new(contact: ContactRecord) -> Self {
        Self::with_contacts(vec![contact])
    }

    fn with_contacts(contacts: Vec<ContactRecord>) -> Self {
        Self {
            contacts: Mutex::new(contacts),
            fail_update: AtomicBool::new(false),
            fail_update_after_commit: AtomicBool::new(false),
            fail_delete_after_commit: AtomicBool::new(false),
            fail_list: AtomicBool::new(false),
            block_update: AtomicBool::new(false),
            block_delete: AtomicBool::new(false),
            create_calls: AtomicUsize::new(0),
            update_calls: AtomicUsize::new(0),
            update_entered: Notify::new(),
            release_update: Notify::new(),
            delete_entered: Notify::new(),
            release_delete: Notify::new(),
        }
    }
}

#[derive(Default)]
struct TestAdmissionControl {
    suspensions: Mutex<HashMap<PeerId, HashSet<TrustBarrierOwnerId>>>,
    resume_attempts: Mutex<HashMap<PeerId, usize>>,
    fail_resume: AtomicBool,
}

#[derive(Default)]
struct CancellationAdmissionControl {
    suspended: Mutex<HashSet<(PeerId, TrustBarrierOwnerId)>>,
    block_next_ensure: AtomicBool,
    block_next_release: AtomicBool,
    ensure_applied: Notify,
    release_applied: Notify,
    finish_ensure: Notify,
    finish_release: Notify,
}

impl CancellationAdmissionControl {
    fn is_suspended(&self, peer_id: &PeerId) -> bool {
        self.suspended.lock().unwrap().iter().any(|(candidate, _)| candidate == peer_id)
    }
}

#[async_trait]
impl AdmissionControl for CancellationAdmissionControl {
    async fn ensure_trust_suspended(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), peer_session::Error> {
        self.suspended.lock().unwrap().insert((peer_id, owner_id));
        if self.block_next_ensure.swap(false, Ordering::SeqCst) {
            self.ensure_applied.notify_one();
            self.finish_ensure.notified().await;
        }
        Ok(())
    }

    async fn release_trust_suspension(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), peer_session::Error> {
        self.suspended.lock().unwrap().remove(&(peer_id, owner_id));
        if self.block_next_release.swap(false, Ordering::SeqCst) {
            self.release_applied.notify_one();
            self.finish_release.notified().await;
        }
        Ok(())
    }
}

impl TestAdmissionControl {
    fn suspension_count(&self, peer_id: &PeerId) -> usize {
        self.suspensions.lock().unwrap().get(peer_id).map(HashSet::len).unwrap_or_default()
    }

    fn resume_attempts(&self, peer_id: &PeerId) -> usize {
        self.resume_attempts.lock().unwrap().get(peer_id).copied().unwrap_or_default()
    }
}

#[async_trait]
impl AdmissionControl for TestAdmissionControl {
    async fn ensure_trust_suspended(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), peer_session::Error> {
        self.suspensions.lock().unwrap().entry(peer_id).or_default().insert(owner_id);
        Ok(())
    }

    async fn release_trust_suspension(
        &self,
        peer_id: PeerId,
        owner_id: TrustBarrierOwnerId,
    ) -> Result<(), peer_session::Error> {
        *self.resume_attempts.lock().unwrap().entry(peer_id.clone()).or_default() += 1;
        if self.fail_resume.load(Ordering::SeqCst) {
            return Err(peer_session::Error::OperationTimeout);
        }
        let remove_peer =
            self.suspensions.lock().unwrap().get_mut(&peer_id).is_some_and(|owners| {
                owners.remove(&owner_id);
                owners.is_empty()
            });
        if remove_peer {
            self.suspensions.lock().unwrap().remove(&peer_id);
        }
        Ok(())
    }
}

#[async_trait]
impl Store for TestContactsStore {
    async fn list(&self) -> Result<Vec<ContactRecord>, StoreError> {
        if self.fail_list.swap(false, Ordering::SeqCst) {
            return Err(forced_store_error("forced reconciliation failure"));
        }
        Ok(self.contacts.lock().unwrap().clone())
    }

    async fn create(
        &self,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError> {
        self.create_calls.fetch_add(1, Ordering::SeqCst);
        let mut contacts = self.contacts.lock().unwrap();
        let id = contacts.iter().map(|contact| contact.id).max().unwrap_or_default() + 1;
        let now = Utc::now();
        let model = ContactRecord {
            id,
            peer_id,
            name,
            trusted_public_key,
            created_at: now,
            updated_at: now,
        };
        contacts.push(model.clone());
        Ok(model)
    }

    async fn delete(&self, id: i64) -> Result<(), StoreError> {
        if self.block_delete.swap(false, Ordering::SeqCst) {
            self.delete_entered.notify_one();
            self.release_delete.notified().await;
        }
        let mut contacts = self.contacts.lock().unwrap();
        let before = contacts.len();
        contacts.retain(|contact| contact.id != id);
        if contacts.len() == before {
            return Err(StoreError::NotFound { id });
        }
        drop(contacts);
        if self.fail_delete_after_commit.swap(false, Ordering::SeqCst) {
            return Err(forced_store_error("forced post-commit delete failure"));
        }
        Ok(())
    }

    async fn update(
        &self,
        id: i64,
        peer_id: String,
        name: String,
        trusted_public_key: String,
    ) -> Result<ContactRecord, StoreError> {
        self.update_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_update.load(Ordering::SeqCst) {
            return Err(forced_store_error("forced update failure"));
        }
        if self.block_update.swap(false, Ordering::SeqCst) {
            self.update_entered.notify_one();
            self.release_update.notified().await;
        }
        let mut contacts = self.contacts.lock().unwrap();
        let contact = contacts
            .iter_mut()
            .find(|contact| contact.id == id)
            .ok_or(StoreError::NotFound { id })?;
        contact.peer_id = peer_id;
        contact.name = name;
        contact.trusted_public_key = trusted_public_key;
        contact.updated_at = Utc::now();
        let committed = contact.clone();
        drop(contacts);
        if self.fail_update_after_commit.swap(false, Ordering::SeqCst) {
            return Err(forced_store_error("forced post-commit update failure"));
        }
        Ok(committed)
    }
}

fn forced_store_error(message: &str) -> StoreError {
    StoreError::storage(std::io::Error::other(message.to_owned()))
}

struct NoEndpoints;

#[async_trait]
impl peer_session::EndpointResolver for NoEndpoints {
    async fn endpoint_hints_for(
        &self,
        _peer_id: &PeerId,
    ) -> Result<Arc<[std::net::SocketAddr]>, peer_session::Error> {
        Ok(Arc::from([]))
    }
}

fn contact_model(peer_id: &PeerId, public_key: String) -> ContactRecord {
    let now = Utc::now();
    ContactRecord {
        id: 1,
        peer_id: peer_id.to_string(),
        name: "Remote".into(),
        trusted_public_key: public_key,
        created_at: now,
        updated_at: now,
    }
}

fn verified_identity(peer_id: &PeerId) -> VerifiedPeerIdentity {
    Invite::new(peer_id.clone(), LocalPeerIdentity::generate().public_key_base64())
        .unwrap()
        .confirm()
}

async fn start_coordinator(
    store: Arc<TestContactsStore>,
) -> (peer_session::PeerSessionService, ContactsService, PeerId, PeerId) {
    let remote_peer = PeerId::new("remote").unwrap();
    let local_peer = PeerId::new("local").unwrap();
    let contacts = Arc::new(Directory::new(store));
    contacts.refresh().await.unwrap();
    let mut config = peer_session::Config::new(contacts.clone(), Arc::new(NoEndpoints));
    config.local_peer_id = Some(local_peer.clone());
    let sessions = peer_session::PeerSessionService::start(config).await.unwrap();
    let coordinator = ContactsService::new(contacts, sessions.service_handle(), local_peer.clone());
    (sessions, coordinator, remote_peer, local_peer)
}

#[tokio::test]
async fn successful_delete_releases_the_barrier_after_removing_trust() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store).await;

    assert!(coordinator.delete(1).await.unwrap().projection.contacts.is_empty());
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotTrusted(remote_peer))
    );

    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn post_commit_delete_error_reconciles_absence_before_resuming_admission() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    store.fail_delete_after_commit.store(true, Ordering::SeqCst);
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store).await;

    let outcome = coordinator.delete(1).await.unwrap();

    assert!(outcome.projection.contacts.is_empty());
    assert_eq!(outcome.admission_warning, None);
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotTrusted(remote_peer))
    );
    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn committed_delete_returns_empty_projection_when_admission_recovery_is_unavailable() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    store.block_delete.store(true, Ordering::SeqCst);
    let (mut sessions, coordinator, _, _) = start_coordinator(store.clone()).await;
    let delete_service = coordinator.clone();
    let delete = tokio::spawn(async move { delete_service.delete(1).await });
    store.delete_entered.notified().await;

    sessions.shutdown(ShutdownContext::default()).await.unwrap();
    store.release_delete.notify_one();
    let outcome = delete.await.unwrap().unwrap();

    assert!(outcome.projection.contacts.is_empty());
    assert_eq!(outcome.admission_warning, Some(peer_session::Error::ServiceStopped));
}

#[tokio::test]
async fn failed_key_update_restores_peer_admission() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    store.fail_update.store(true, Ordering::SeqCst);
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store).await;

    let result = coordinator.update_verified_identity(1, verified_identity(&remote_peer)).await;
    assert!(matches!(result, Err(Error::Contact(_))));
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotNearby(remote_peer))
    );

    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn key_update_is_bracketed_by_suspension_until_persistence_finishes() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    store.block_update.store(true, Ordering::SeqCst);
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store.clone()).await;
    let handle = sessions.service_handle();

    let update_service = coordinator.clone();
    let updated_identity = verified_identity(&remote_peer);
    let update =
        tokio::spawn(
            async move { update_service.update_verified_identity(1, updated_identity).await },
        );
    store.update_entered.notified().await;
    assert_eq!(
        handle.connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerSuspended(remote_peer.clone()))
    );

    store.release_update.notify_one();
    update.await.unwrap().unwrap();
    assert_eq!(
        handle.connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotNearby(remote_peer))
    );

    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn cancelled_acquire_keeps_recorded_intent_and_retry_reasserts_the_barrier() {
    let remote_peer = PeerId::new("remote").unwrap();
    let local_peer = PeerId::new("local").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let contacts = Arc::new(Directory::new(store.clone()));
    contacts.refresh().await.unwrap();
    let admission = Arc::new(CancellationAdmissionControl::default());
    let coordinator =
        ContactsService::new_with_admission_control(contacts, admission.clone(), local_peer);
    let identity = verified_identity(&remote_peer);

    admission.block_next_ensure.store(true, Ordering::SeqCst);
    let cancelled_service = coordinator.clone();
    let cancelled_identity = identity.clone();
    let cancelled = tokio::spawn(async move {
        cancelled_service.update_verified_identity(1, cancelled_identity).await
    });
    admission.ensure_applied.notified().await;
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    assert!(admission.is_suspended(&remote_peer));
    assert_eq!(store.update_calls.load(Ordering::SeqCst), 0);

    store.block_update.store(true, Ordering::SeqCst);
    let retry_service = coordinator.clone();
    let retry =
        tokio::spawn(async move { retry_service.update_verified_identity(1, identity).await });
    store.update_entered.notified().await;
    assert!(admission.is_suspended(&remote_peer));

    store.release_update.notify_one();
    retry.await.unwrap().unwrap();
    assert!(!admission.is_suspended(&remote_peer));
    assert_eq!(store.update_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelled_release_cannot_make_a_retry_write_without_a_barrier() {
    let remote_peer = PeerId::new("remote").unwrap();
    let local_peer = PeerId::new("local").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    store.fail_update.store(true, Ordering::SeqCst);
    let contacts = Arc::new(Directory::new(store.clone()));
    contacts.refresh().await.unwrap();
    let admission = Arc::new(CancellationAdmissionControl::default());
    let coordinator =
        ContactsService::new_with_admission_control(contacts, admission.clone(), local_peer);
    let identity = verified_identity(&remote_peer);

    admission.block_next_release.store(true, Ordering::SeqCst);
    let cancelled_service = coordinator.clone();
    let cancelled_identity = identity.clone();
    let cancelled = tokio::spawn(async move {
        cancelled_service.update_verified_identity(1, cancelled_identity).await
    });
    admission.release_applied.notified().await;
    assert!(!admission.is_suspended(&remote_peer));
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());

    store.fail_update.store(false, Ordering::SeqCst);
    store.block_update.store(true, Ordering::SeqCst);
    let retry_service = coordinator.clone();
    let retry =
        tokio::spawn(async move { retry_service.update_verified_identity(1, identity).await });
    store.update_entered.notified().await;
    assert!(admission.is_suspended(&remote_peer));

    store.release_update.notify_one();
    retry.await.unwrap().unwrap();
    assert!(!admission.is_suspended(&remote_peer));
    assert_eq!(store.update_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn local_identity_is_rejected_before_persistence_or_session_mutation() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let (mut sessions, coordinator, _, local_peer) = start_coordinator(store.clone()).await;
    let snapshot_before = sessions.service_handle().snapshot();

    let result = coordinator.create("Myself".into(), verified_identity(&local_peer)).await;

    assert!(matches!(
        result,
        Err(Error::SelfIdentity { peer_id }) if peer_id == local_peer
    ));
    assert_eq!(store.create_calls.load(Ordering::SeqCst), 0);
    assert_eq!(sessions.service_handle().snapshot(), snapshot_before);

    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn replacement_identity_must_belong_to_the_contacts_existing_peer() {
    let remote_peer = PeerId::new("remote").unwrap();
    let other_peer = PeerId::new("other").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let (mut sessions, coordinator, _, _) = start_coordinator(store.clone()).await;
    let snapshot_before = sessions.service_handle().snapshot();

    let result = coordinator.update_verified_identity(1, verified_identity(&other_peer)).await;

    assert!(matches!(
        result,
        Err(Error::PeerIdentityMismatch {
            contact_id: 1,
            expected,
            actual,
        }) if expected == remote_peer && actual == other_peer
    ));
    assert_eq!(store.update_calls.load(Ordering::SeqCst), 0);
    assert_eq!(sessions.service_handle().snapshot(), snapshot_before);
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotNearby(remote_peer))
    );

    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn post_commit_update_error_reconciles_the_new_key_before_resuming_admission() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    store.fail_update_after_commit.store(true, Ordering::SeqCst);
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store).await;
    let identity = verified_identity(&remote_peer);
    let intended_key = identity.public_key_base64().to_owned();

    let outcome = coordinator.update_verified_identity(1, identity).await.unwrap();

    assert_eq!(outcome.projection.contacts[0].trusted_public_key, intended_key);
    assert_eq!(outcome.admission_warning, None);
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotNearby(remote_peer))
    );
    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn indeterminate_post_commit_update_stays_suspended_instead_of_trusting_the_stale_key() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store.clone()).await;
    store.fail_update_after_commit.store(true, Ordering::SeqCst);
    store.fail_list.store(true, Ordering::SeqCst);

    let identity = verified_identity(&remote_peer);
    let result = coordinator.update_verified_identity(1, identity.clone()).await;

    assert!(matches!(result, Err(Error::OutcomeUnknown { .. })));
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerSuspended(remote_peer.clone()))
    );

    let outcome = coordinator.update_verified_identity(1, identity).await.unwrap();
    assert_eq!(outcome.admission_warning, None);
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotNearby(remote_peer))
    );
    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn authoritative_refresh_releases_a_definitively_applied_rotation_barrier() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store.clone()).await;
    store.fail_update_after_commit.store(true, Ordering::SeqCst);
    store.fail_list.store(true, Ordering::SeqCst);
    let identity = verified_identity(&remote_peer);
    let intended_key = identity.public_key_base64().to_owned();

    assert!(matches!(
        coordinator.update_verified_identity(1, identity).await,
        Err(Error::OutcomeUnknown { .. })
    ));
    let outcome = coordinator.refresh().await.unwrap();

    assert_eq!(outcome.projection.contacts[0].trusted_public_key, intended_key);
    assert!(outcome.admission_warnings.is_empty());
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotNearby(remote_peer))
    );
    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn refresh_surfaces_every_retained_barrier_and_retries_their_recovery() {
    let peer_a = PeerId::new("peer-a").unwrap();
    let peer_b = PeerId::new("peer-b").unwrap();
    let local_peer = PeerId::new("local").unwrap();
    let contact_a = contact_model(&peer_a, LocalPeerIdentity::generate().public_key_base64());
    let mut contact_b = contact_model(&peer_b, LocalPeerIdentity::generate().public_key_base64());
    contact_b.id = 2;
    contact_b.name = "Remote B".into();
    let store = Arc::new(TestContactsStore::with_contacts(vec![contact_a, contact_b]));
    let contacts = Arc::new(Directory::new(store.clone()));
    contacts.refresh().await.unwrap();
    let admission = Arc::new(TestAdmissionControl::default());
    let coordinator =
        ContactsService::new_with_admission_control(contacts, admission.clone(), local_peer);

    for (id, peer_id) in [(1, &peer_a), (2, &peer_b)] {
        store.fail_update_after_commit.store(true, Ordering::SeqCst);
        store.fail_list.store(true, Ordering::SeqCst);
        assert!(matches!(
            coordinator.update_verified_identity(id, verified_identity(peer_id)).await,
            Err(Error::OutcomeUnknown { .. })
        ));
        assert_eq!(admission.suspension_count(peer_id), 1);
    }

    admission.fail_resume.store(true, Ordering::SeqCst);
    let warning_outcome = coordinator.refresh().await.unwrap();
    let mut warned_peers = warning_outcome
        .admission_warnings
        .iter()
        .map(|warning| {
            assert_eq!(warning.error, peer_session::Error::OperationTimeout);
            warning.peer_id.as_str().to_owned()
        })
        .collect::<Vec<_>>();
    warned_peers.sort();
    assert_eq!(warned_peers, ["peer-a", "peer-b"]);
    assert_eq!(admission.suspension_count(&peer_a), 1);
    assert_eq!(admission.suspension_count(&peer_b), 1);

    admission.fail_resume.store(false, Ordering::SeqCst);
    let recovered = coordinator.refresh().await.unwrap();
    assert!(recovered.admission_warnings.is_empty());
    assert_eq!(admission.suspension_count(&peer_a), 0);
    assert_eq!(admission.suspension_count(&peer_b), 0);
    assert_eq!(admission.resume_attempts(&peer_a), 2);
    assert_eq!(admission.resume_attempts(&peer_b), 2);
}

#[tokio::test]
async fn indeterminate_committed_delete_can_be_retried_without_double_suspension() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store.clone()).await;
    store.fail_delete_after_commit.store(true, Ordering::SeqCst);
    store.fail_list.store(true, Ordering::SeqCst);

    assert!(matches!(coordinator.delete(1).await, Err(Error::OutcomeUnknown { .. })));
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerSuspended(remote_peer.clone()))
    );

    let outcome = coordinator.delete(1).await.unwrap();
    assert!(outcome.projection.contacts.is_empty());
    assert_eq!(outcome.admission_warning, None);
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotTrusted(remote_peer))
    );
    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn create_after_ambiguous_delete_reconciles_and_releases_the_old_barrier() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store.clone()).await;
    store.fail_delete_after_commit.store(true, Ordering::SeqCst);
    store.fail_list.store(true, Ordering::SeqCst);

    assert!(matches!(coordinator.delete(1).await, Err(Error::OutcomeUnknown { .. })));
    let outcome =
        coordinator.create("Remote again".into(), verified_identity(&remote_peer)).await.unwrap();

    assert_eq!(outcome.projection.contacts.len(), 1);
    assert_eq!(outcome.projection.contacts[0].peer_id, remote_peer);
    assert_eq!(outcome.admission_warning, None);
    assert_eq!(
        sessions.service_handle().connect(remote_peer.clone()).await,
        Err(peer_session::Error::PeerNotNearby(remote_peer))
    );
    sessions.shutdown(ShutdownContext::default()).await.unwrap();
}

#[tokio::test]
async fn committed_rotation_returns_its_projection_when_admission_recovery_is_unavailable() {
    let remote_peer = PeerId::new("remote").unwrap();
    let store = Arc::new(TestContactsStore::new(contact_model(
        &remote_peer,
        LocalPeerIdentity::generate().public_key_base64(),
    )));
    store.block_update.store(true, Ordering::SeqCst);
    let (mut sessions, coordinator, remote_peer, _) = start_coordinator(store.clone()).await;
    let identity = verified_identity(&remote_peer);
    let intended_key = identity.public_key_base64().to_owned();
    let update_service = coordinator.clone();
    let update =
        tokio::spawn(async move { update_service.update_verified_identity(1, identity).await });
    store.update_entered.notified().await;

    sessions.shutdown(ShutdownContext::default()).await.unwrap();
    store.release_update.notify_one();
    let outcome = update.await.unwrap().unwrap();

    assert_eq!(outcome.projection.contacts[0].trusted_public_key, intended_key);
    assert_eq!(outcome.admission_warning, Some(peer_session::Error::ServiceStopped));
}

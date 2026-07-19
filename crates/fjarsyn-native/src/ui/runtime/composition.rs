use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use fjarsyn_core::{
    config::Config,
    peer_session::{
        PeerId, PeerSessionPhase, PeerSessionService, PeerSessionServiceConfig, RemoteShareState,
        SessionId,
    },
    presence::{PresenceService, PresenceServiceConfig},
    repositories::{ContactsRepository, MessagesRepository},
    services::{
        codec_service::{self, CodecDirectionState, Handle, Service},
        contact_trust_service::ContactTrustService,
        contacts_service::ContactsService,
        messaging_service::{
            MessagingService, MessagingServiceConfig, MessagingServiceLimits, MessagingSnapshot,
            SESSION_EVENT_CAPACITY,
        },
    },
};
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
};

use super::{
    ApplicationHandles, ApplicationOwners, ApplicationRuntime, MediaCodecDirection, MediaEvent,
    RuntimeEvent, SessionMediaService, ShareMediaBinding, resolvers::DeferredEndpointResolver,
};

pub async fn start_application_runtime(
    mut config: Config,
    event_tx: mpsc::Sender<RuntimeEvent>,
) -> Result<ApplicationRuntime, String> {
    let database = super::initialize_database().await?;
    let contacts =
        Arc::new(ContactsService::new(Arc::new(ContactsRepository::new(database.clone()))));
    if let Err(error) = contacts.refresh().await {
        database.close().await;
        return Err(error.to_string());
    }

    let endpoints = Arc::new(DeferredEndpointResolver::default());
    let (session_event_tx, session_event_rx) = mpsc::channel(SESSION_EVENT_CAPACITY);
    let mut session_config = PeerSessionServiceConfig::new(contacts.clone(), endpoints.clone());
    session_config.mandatory_event_sink = Some(session_event_tx);
    session_config.local_peer_id =
        match config.identity.peer_id.as_deref().map(PeerId::new).transpose() {
            Ok(peer_id) => peer_id,
            Err(error) => {
                database.close().await;
                return Err(error.to_string());
            }
        };
    session_config.identity_keypair = config.identity.signing_key.clone();
    session_config.max_depacket_latency =
        Duration::from_millis(u64::from(config.network.max_depacket_latency));

    let sessions = match PeerSessionService::start(session_config).await {
        Ok(sessions) => sessions,
        Err(error) => {
            database.close().await;
            return Err(error.to_string());
        }
    };
    let local_peer_id = sessions.local_peer_id().clone();
    let local_public_key = sessions.local_public_key();
    config.identity.peer_id = Some(local_peer_id.to_string());
    config.identity.signing_key = Some(sessions.stored_identity_keypair());
    if let Err(error) = config.save() {
        let _ = sessions.shutdown().await;
        database.close().await;
        return Err(format!("failed to persist local identity: {error}"));
    }

    let presence = match PresenceService::start(PresenceServiceConfig::new(
        local_peer_id.to_string(),
        sessions.signaling_port(),
    )) {
        Ok(presence) => presence,
        Err(error) => {
            let _ = sessions.shutdown().await;
            database.close().await;
            return Err(error.to_string());
        }
    };
    let presence_handle = presence.handle();
    endpoints.install(presence_handle.clone());

    let session_handle = sessions.handle();
    let contact_trust =
        Arc::new(ContactTrustService::new(contacts, session_handle.clone(), local_peer_id.clone()));
    let messaging = match MessagingService::start(MessagingServiceConfig {
        repository: Arc::new(MessagesRepository::new(database.clone())),
        sessions: session_handle.clone(),
        session_events: session_event_rx,
        limits: MessagingServiceLimits::default(),
    })
    .await
    {
        Ok(messaging) => messaging,
        Err(error) => {
            let _ = presence.shutdown().await;
            let _ = sessions.shutdown().await;
            database.close().await;
            return Err(error.to_string());
        }
    };
    let messaging_handle = messaging.handle();
    let (codecs, codec_handle) = Service::start(codec_service::Config::default());
    let media_config = Arc::new(std::sync::RwLock::new(config.clone()));
    let media = Arc::new(Mutex::new(SessionMediaService::new(
        event_tx.clone(),
        session_handle.clone(),
        codec_handle.clone(),
    )));

    let event_workers = spawn_event_workers(
        &session_handle,
        &presence_handle,
        &messaging_handle,
        &codec_handle,
        media.clone(),
        media_config.clone(),
        event_tx,
    );
    let handles = ApplicationHandles {
        contacts: contact_trust,
        sessions: session_handle,
        presence: presence_handle,
        messaging: messaging_handle,
        media,
    };

    Ok(ApplicationRuntime::new(
        handles,
        local_peer_id,
        local_public_key,
        config,
        media_config,
        ApplicationOwners { database, codecs, sessions, presence, messaging, event_workers },
    ))
}

fn spawn_event_workers(
    sessions: &fjarsyn_core::peer_session::PeerSessionServiceHandle,
    presence: &fjarsyn_core::presence::PresenceHandle,
    messaging: &fjarsyn_core::services::messaging_service::MessagingServiceHandle,
    codecs: &Handle,
    media: Arc<Mutex<SessionMediaService>>,
    media_config: Arc<std::sync::RwLock<Config>>,
    event_tx: mpsc::Sender<RuntimeEvent>,
) -> Vec<JoinHandle<()>> {
    let mut workers = Vec::new();

    let mut codec_snapshots = codecs.subscribe();
    let codec_media = media.clone();
    let codec_tx = event_tx.clone();
    workers.push(tokio::spawn(async move {
        while codec_snapshots.changed().await.is_ok() {
            let snapshot = codec_snapshots.borrow_and_update().clone();
            let directions = [
                (&snapshot.encode, MediaCodecDirection::Encoder),
                (&snapshot.decode, MediaCodecDirection::Decoder),
            ];
            for (state, direction) in directions {
                if !matches!(state, CodecDirectionState::RestartRequired(_)) {
                    continue;
                }
                let transitioned = codec_media.lock().await.require_codec_restart(direction);
                if transitioned
                    && codec_tx
                        .send(RuntimeEvent::Media(MediaEvent::CodecRestartRequired { direction }))
                        .await
                        .is_err()
                {
                    return;
                }
            }
        }
    }));

    let mut session_snapshots = sessions.subscribe();
    let mut session_events = sessions.events();
    let session_commands = sessions.clone();
    let session_tx = event_tx.clone();
    workers.push(tokio::spawn(async move {
        let mut projected_sessions = BTreeSet::new();
        let mut projected_remote_shares = BTreeMap::new();
        let mut remote_retry_after = BTreeMap::new();
        let mut media_reconcile_tick = tokio::time::interval(Duration::from_millis(250));
        media_reconcile_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = session_snapshots.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let snapshot = session_snapshots.borrow_and_update().clone();
                    reconcile_media_sessions(
                        &session_commands,
                        &media,
                        &media_config,
                        &snapshot,
                        &mut projected_sessions,
                        &mut projected_remote_shares,
                        &mut remote_retry_after,
                    ).await;
                    if session_tx.send(RuntimeEvent::Sessions(snapshot)).await.is_err() {
                        break;
                    }
                }
                event = session_events.recv() => match event {
                    Ok(event) => {
                        // Semantic UI events are best-effort. Immutable snapshots
                        // are the authoritative projection and cannot backpressure
                        // the session actor.
                        let _ = session_tx.try_send(RuntimeEvent::SessionEvent(event));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!("native session projection lagged by {skipped} events; reconciling snapshot");
                        let snapshot = session_commands.snapshot();
                        reconcile_media_sessions(
                            &session_commands,
                            &media,
                            &media_config,
                            &snapshot,
                            &mut projected_sessions,
                            &mut projected_remote_shares,
                            &mut remote_retry_after,
                        ).await;
                        let _ = session_tx.send(RuntimeEvent::Sessions(snapshot)).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                _ = media_reconcile_tick.tick() => {
                    let snapshot = session_commands.snapshot();
                    reconcile_media_sessions(
                        &session_commands,
                        &media,
                        &media_config,
                        &snapshot,
                        &mut projected_sessions,
                        &mut projected_remote_shares,
                        &mut remote_retry_after,
                    ).await;
                }
            }
        }
    }));

    let mut presence_snapshots = presence.subscribe();
    let presence_tx = event_tx.clone();
    workers.push(tokio::spawn(async move {
        while presence_snapshots.changed().await.is_ok() {
            let snapshot = presence_snapshots.borrow_and_update().clone();
            if presence_tx.send(RuntimeEvent::Presence(snapshot)).await.is_err() {
                break;
            }
        }
    }));

    let mut messaging_snapshots = messaging.subscribe();
    let messaging_tx = event_tx.clone();
    workers.push(tokio::spawn(async move {
        let mut revision = 0_u64;
        while messaging_snapshots.changed().await.is_ok() {
            revision = revision.wrapping_add(1);
            let snapshot = messaging_snapshots.borrow_and_update().clone();
            if send_messaging_snapshot(&messaging_tx, revision, snapshot).await.is_err() {
                break;
            }
        }
    }));

    let mut messaging_events = messaging.events();
    workers.push(tokio::spawn(async move {
        loop {
            match messaging_events.recv().await {
                Ok(event) => {
                    if event_tx.send(RuntimeEvent::MessagingEvent(event)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("native messaging event projection lagged by {skipped} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));

    workers
}

async fn reconcile_media_sessions(
    sessions: &fjarsyn_core::peer_session::PeerSessionServiceHandle,
    media: &Arc<Mutex<SessionMediaService>>,
    media_config: &Arc<std::sync::RwLock<Config>>,
    snapshot: &fjarsyn_core::peer_session::PeerSessionServiceSnapshot,
    projected_sessions: &mut BTreeSet<SessionId>,
    projected_remote_shares: &mut BTreeMap<SessionId, ShareMediaBinding>,
    remote_retry_after: &mut BTreeMap<SessionId, (ShareMediaBinding, tokio::time::Instant)>,
) {
    let local_plan = media.lock().await.local_reconciliation();
    if let Some(binding) = local_plan.teardown_native {
        media.lock().await.teardown_local(binding).await;
    }
    for binding in local_plan.stop_core {
        if let Err(error) = sessions.stop_screen_share(binding.session_id, binding.share_id).await
            && !matches!(
                error,
                fjarsyn_core::peer_session::PeerSessionError::SessionNotFound(_)
                    | fjarsyn_core::peer_session::PeerSessionError::ShareMismatch(_)
            )
        {
            tracing::debug!(
                session_id = %binding.session_id,
                share_id = %binding.share_id,
                %error,
                "local screen-share stop remains pending reconciliation"
            );
        }
    }

    let retained = snapshot
        .sessions
        .iter()
        .filter(|session| retains_media_session(session.phase))
        .map(|session| session.session_id)
        .collect::<BTreeSet<_>>();
    let connected = snapshot
        .sessions
        .iter()
        .filter(|session| permits_media_capability_creation(session.phase))
        .map(|session| session.session_id)
        .collect::<BTreeSet<_>>();

    for session_id in projected_sessions.difference(&retained).copied().collect::<Vec<_>>() {
        media.lock().await.stop_session(session_id).await;
        projected_remote_shares.remove(&session_id);
        remote_retry_after.remove(&session_id);
    }

    let remote_shares = snapshot
        .sessions
        .iter()
        .filter(|session| retains_media_session(session.phase))
        .filter_map(|session| match session.remote_share {
            RemoteShareState::Active { share_id, epoch } => {
                Some((session.session_id, ShareMediaBinding { share_id, epoch }))
            }
            RemoteShareState::Inactive => None,
        })
        .collect::<BTreeMap<_, _>>();

    let replaced_shares = projected_remote_shares
        .iter()
        .filter_map(|(&session_id, previous_binding)| {
            (remote_shares.get(&session_id) != Some(previous_binding)).then_some(session_id)
        })
        .collect::<Vec<_>>();
    for session_id in replaced_shares {
        media.lock().await.stop_remote(session_id).await;
        projected_remote_shares.remove(&session_id);
        remote_retry_after.remove(&session_id);
    }

    // A watchdog timeout quarantines decoding for the lifetime of this
    // process. Do not retain receivers or let reconciliation turn that sticky
    // state into a decoder-recreation loop.
    if media.lock().await.decoder_restart_required() {
        projected_remote_shares.clear();
        remote_retry_after.clear();
        *projected_sessions = retained;
        return;
    }

    // Own a receiver for every connected session before ShareStarted can be
    // projected. The first core receiver retains any samples that raced the
    // service snapshot; decoding begins only for the exact ShareId/epoch.
    for &session_id in &connected {
        if let Err(error) = ensure_remote_standby(sessions, media, session_id).await {
            tracing::debug!(
                %session_id,
                %error,
                "remote video standby subscription will be retried"
            );
        }
    }

    for (&session_id, &binding) in &remote_shares {
        if projected_remote_shares.get(&session_id) == Some(&binding) {
            if media.lock().await.remote_is_running(session_id, binding) {
                continue;
            }
            // A worker that terminated for this exact authenticated share is
            // terminal for that share. Wait for a new ShareId/epoch instead of
            // silently creating replacement codec workers behind the UI.
            continue;
        }
        if !connected.contains(&session_id) {
            continue;
        }
        if remote_retry_after.get(&session_id).is_some_and(|(failed_binding, retry_at)| {
            *failed_binding == binding && tokio::time::Instant::now() < *retry_at
        }) {
            continue;
        }
        if let Err(error) = ensure_remote_standby(sessions, media, session_id).await {
            remote_retry_after.insert(
                session_id,
                (binding, tokio::time::Instant::now() + Duration::from_secs(1)),
            );
            tracing::debug!(
                %session_id,
                share_id = %binding.share_id,
                share_epoch = binding.epoch.value(),
                %error,
                "remote decoder is waiting for standby source"
            );
            continue;
        }
        let config = media_config.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        match media.lock().await.start_remote(session_id, binding, config).await {
            Ok(()) => {
                projected_remote_shares.insert(session_id, binding);
                remote_retry_after.remove(&session_id);
            }
            Err(error) => {
                projected_remote_shares.insert(session_id, binding);
                remote_retry_after.remove(&session_id);
                tracing::warn!("failed to start remote video: {error}");
            }
        }
    }
    remote_retry_after
        .retain(|session_id, (binding, _)| remote_shares.get(session_id) == Some(binding));
    *projected_sessions = retained;
}

fn retains_media_session(phase: PeerSessionPhase) -> bool {
    matches!(phase, PeerSessionPhase::Connected | PeerSessionPhase::Reconnecting)
}

fn permits_media_capability_creation(phase: PeerSessionPhase) -> bool {
    phase == PeerSessionPhase::Connected
}

async fn ensure_remote_standby(
    sessions: &fjarsyn_core::peer_session::PeerSessionServiceHandle,
    media: &Arc<Mutex<SessionMediaService>>,
    session_id: SessionId,
) -> Result<(), String> {
    if media.lock().await.remote_receiver_ready(session_id) {
        return Ok(());
    }
    let source =
        sessions.subscribe_remote_video(session_id).await.map_err(|error| error.to_string())?;
    let mut media = media.lock().await;
    media.install_standby_remote(session_id, source);
    media
        .remote_receiver_ready(session_id)
        .then_some(())
        .ok_or_else(|| "remote standby source was not retained".into())
}

async fn send_messaging_snapshot(
    event_tx: &mpsc::Sender<RuntimeEvent>,
    revision: u64,
    snapshot: MessagingSnapshot,
) -> Result<(), ()> {
    let conversations = snapshot
        .conversations
        .iter()
        .map(|(peer_id, messages)| (peer_id.clone(), messages.clone()))
        .collect::<BTreeMap<_, _>>();
    event_tx
        .send(RuntimeEvent::Messaging {
            revision,
            summaries: snapshot.summaries,
            conversations: Arc::new(conversations),
        })
        .await
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use fjarsyn_core::peer_session::{PeerSessionPhase, SessionId, ShareEpoch, ShareId};

    use super::{ShareMediaBinding, permits_media_capability_creation, retains_media_session};

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    struct MediaWork {
        stop_session: usize,
        stop_remote: usize,
        install_standby: usize,
        start_remote: usize,
    }

    #[derive(Default)]
    struct ReconciliationProbe {
        projected_sessions: BTreeSet<SessionId>,
        projected_remote_shares: BTreeMap<SessionId, ShareMediaBinding>,
        running_remote: BTreeMap<SessionId, ShareMediaBinding>,
        receiver_ready: BTreeSet<SessionId>,
        work: MediaWork,
    }

    impl ReconciliationProbe {
        fn reconcile(&mut self, session: Option<(SessionId, PeerSessionPhase, ShareMediaBinding)>) {
            let retained = session
                .filter(|(_, phase, _)| retains_media_session(*phase))
                .map(|(session_id, _, _)| session_id)
                .into_iter()
                .collect::<BTreeSet<_>>();
            let connected = session
                .filter(|(_, phase, _)| permits_media_capability_creation(*phase))
                .map(|(session_id, _, _)| session_id)
                .into_iter()
                .collect::<BTreeSet<_>>();

            for session_id in
                self.projected_sessions.difference(&retained).copied().collect::<Vec<_>>()
            {
                self.work.stop_session += 1;
                self.projected_remote_shares.remove(&session_id);
                self.running_remote.remove(&session_id);
                self.receiver_ready.remove(&session_id);
            }

            let remote_shares = session
                .filter(|(_, phase, _)| retains_media_session(*phase))
                .map(|(session_id, _, binding)| (session_id, binding))
                .into_iter()
                .collect::<BTreeMap<_, _>>();
            let replaced = self
                .projected_remote_shares
                .iter()
                .filter_map(|(&session_id, previous_share_id)| {
                    (remote_shares.get(&session_id) != Some(previous_share_id))
                        .then_some(session_id)
                })
                .collect::<Vec<_>>();
            for session_id in replaced {
                self.work.stop_remote += 1;
                self.running_remote.remove(&session_id);
                self.projected_remote_shares.remove(&session_id);
            }

            for &session_id in &connected {
                if self.receiver_ready.insert(session_id) {
                    self.work.install_standby += 1;
                }
            }

            for (&session_id, &binding) in &remote_shares {
                if self.projected_remote_shares.get(&session_id) == Some(&binding) {
                    continue;
                }
                self.projected_remote_shares.remove(&session_id);
                if !connected.contains(&session_id) {
                    continue;
                }
                if self.receiver_ready.insert(session_id) {
                    self.work.install_standby += 1;
                }
                self.work.start_remote += 1;
                self.running_remote.insert(session_id, binding);
                self.projected_remote_shares.insert(session_id, binding);
            }
            self.projected_sessions = retained;
        }
    }

    #[test]
    fn reconnecting_retains_media_without_creating_capabilities() {
        assert!(retains_media_session(PeerSessionPhase::Connected));
        assert!(retains_media_session(PeerSessionPhase::Reconnecting));
        assert!(permits_media_capability_creation(PeerSessionPhase::Connected));
        assert!(!permits_media_capability_creation(PeerSessionPhase::Reconnecting));
    }

    #[test]
    fn inactive_session_phases_release_media_ownership() {
        for phase in [
            PeerSessionPhase::Requesting,
            PeerSessionPhase::Incoming,
            PeerSessionPhase::Negotiating,
            PeerSessionPhase::Disconnecting,
        ] {
            assert!(!retains_media_session(phase));
            assert!(!permits_media_capability_creation(phase));
        }
    }

    #[test]
    fn reconnecting_preserves_running_media_until_one_terminal_teardown() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let binding = ShareMediaBinding { share_id, epoch: ShareEpoch::FIRST };
        let active = |phase| Some((session_id, phase, binding));
        let mut probe = ReconciliationProbe::default();

        probe.reconcile(active(PeerSessionPhase::Connected));
        assert_eq!(probe.work.install_standby, 1);
        assert_eq!(probe.work.start_remote, 1);
        assert_eq!(probe.running_remote.get(&session_id), Some(&binding));
        probe.work = MediaWork::default();

        probe.reconcile(active(PeerSessionPhase::Reconnecting));
        assert_eq!(probe.work, MediaWork::default());
        assert_eq!(probe.running_remote.get(&session_id), Some(&binding));

        probe.reconcile(active(PeerSessionPhase::Connected));
        assert_eq!(probe.work, MediaWork::default());
        assert_eq!(probe.running_remote.get(&session_id), Some(&binding));

        probe.reconcile(active(PeerSessionPhase::Reconnecting));
        probe.reconcile(None);
        assert_eq!(probe.work, MediaWork { stop_session: 1, ..MediaWork::default() });
        assert!(probe.running_remote.is_empty());

        probe.reconcile(None);
        assert_eq!(probe.work.stop_session, 1);
    }

    #[test]
    fn a_new_epoch_replaces_remote_media_even_if_the_share_id_is_reused() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let epoch_a = ShareEpoch::FIRST;
        let epoch_b = ShareEpoch::try_from(epoch_a.value() + 1).unwrap();
        let binding_a = ShareMediaBinding { share_id, epoch: epoch_a };
        let binding_b = ShareMediaBinding { share_id, epoch: epoch_b };
        let mut probe = ReconciliationProbe::default();

        probe.reconcile(Some((session_id, PeerSessionPhase::Connected, binding_a)));
        probe.work = MediaWork::default();
        probe.reconcile(Some((session_id, PeerSessionPhase::Connected, binding_b)));

        assert_eq!(probe.work.stop_remote, 1);
        assert_eq!(probe.work.start_remote, 1);
        assert_eq!(probe.running_remote.get(&session_id), Some(&binding_b));
    }

    #[test]
    fn failed_decoder_is_not_recreated_for_the_same_share_binding() {
        let session_id = SessionId::new();
        let binding_a = ShareMediaBinding { share_id: ShareId::new(), epoch: ShareEpoch::FIRST };
        let binding_b = ShareMediaBinding {
            share_id: binding_a.share_id,
            epoch: ShareEpoch::try_from(binding_a.epoch.value() + 1).unwrap(),
        };
        let mut probe = ReconciliationProbe::default();

        probe.reconcile(Some((session_id, PeerSessionPhase::Connected, binding_a)));
        probe.running_remote.remove(&session_id);
        probe.work = MediaWork::default();
        probe.reconcile(Some((session_id, PeerSessionPhase::Connected, binding_a)));

        assert_eq!(probe.work, MediaWork::default());

        probe.reconcile(Some((session_id, PeerSessionPhase::Connected, binding_b)));
        assert_eq!(probe.work.stop_remote, 1);
        assert_eq!(probe.work.start_remote, 1);
    }
}

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use fjarsyn_core::{
    config::Config,
    peer_session::{
        PeerId, PeerSessionPhase, PeerSessionService, PeerSessionServiceConfig, RemoteShareState,
        SessionId, ShareId,
    },
    presence::{PresenceService, PresenceServiceConfig},
    repositories::{ContactsRepository, MessagesRepository},
    services::{
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
    ApplicationHandles, ApplicationOwners, ApplicationRuntime, RuntimeEvent, SessionMediaService,
    resolvers::DeferredEndpointResolver,
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
    let media_config = Arc::new(std::sync::RwLock::new(config.clone()));
    let media =
        Arc::new(Mutex::new(SessionMediaService::new(event_tx.clone(), session_handle.clone())));

    let event_workers = spawn_event_workers(
        &session_handle,
        &presence_handle,
        &messaging_handle,
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
        ApplicationOwners { database, sessions, presence, messaging, event_workers },
    ))
}

fn spawn_event_workers(
    sessions: &fjarsyn_core::peer_session::PeerSessionServiceHandle,
    presence: &fjarsyn_core::presence::PresenceHandle,
    messaging: &fjarsyn_core::services::messaging_service::MessagingServiceHandle,
    media: Arc<Mutex<SessionMediaService>>,
    media_config: Arc<std::sync::RwLock<Config>>,
    event_tx: mpsc::Sender<RuntimeEvent>,
) -> Vec<JoinHandle<()>> {
    let mut workers = Vec::new();

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
    projected_remote_shares: &mut BTreeMap<SessionId, ShareId>,
    remote_retry_after: &mut BTreeMap<SessionId, (ShareId, tokio::time::Instant)>,
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

    let connected = snapshot
        .sessions
        .iter()
        .filter(|session| session.phase == PeerSessionPhase::Connected)
        .map(|session| session.session_id)
        .collect::<BTreeSet<_>>();

    for session_id in projected_sessions.difference(&connected).copied().collect::<Vec<_>>() {
        media.lock().await.stop_session(session_id).await;
        projected_remote_shares.remove(&session_id);
        remote_retry_after.remove(&session_id);
    }

    let remote_shares = snapshot
        .sessions
        .iter()
        .filter(|session| session.phase == PeerSessionPhase::Connected)
        .filter_map(|session| match session.remote_share {
            RemoteShareState::Active { share_id } => Some((session.session_id, share_id)),
            RemoteShareState::Inactive => None,
        })
        .collect::<BTreeMap<_, _>>();

    let replaced_shares = projected_remote_shares
        .iter()
        .filter_map(|(&session_id, previous_share_id)| {
            (remote_shares.get(&session_id) != Some(previous_share_id)).then_some(session_id)
        })
        .collect::<Vec<_>>();
    for session_id in replaced_shares {
        media.lock().await.stop_remote(session_id).await;
        projected_remote_shares.remove(&session_id);
        remote_retry_after.remove(&session_id);
    }

    // Own a receiver for every connected session before ShareStarted can be
    // projected. The first core receiver retains any samples that raced the
    // service snapshot; decoding still begins only for the exact active ShareId.
    for &session_id in &connected {
        if let Err(error) = ensure_remote_standby(sessions, media, session_id).await {
            tracing::debug!(
                %session_id,
                %error,
                "remote video standby subscription will be retried"
            );
        }
    }

    for (&session_id, &share_id) in &remote_shares {
        if projected_remote_shares.get(&session_id) == Some(&share_id) {
            if media.lock().await.remote_is_running(session_id, share_id) {
                continue;
            }
            projected_remote_shares.remove(&session_id);
        }
        if remote_retry_after.get(&session_id).is_some_and(|(failed_share_id, retry_at)| {
            *failed_share_id == share_id && tokio::time::Instant::now() < *retry_at
        }) {
            continue;
        }
        if let Err(error) = ensure_remote_standby(sessions, media, session_id).await {
            remote_retry_after.insert(
                session_id,
                (share_id, tokio::time::Instant::now() + Duration::from_secs(1)),
            );
            tracing::debug!(%session_id, %share_id, %error, "remote decoder is waiting for standby source");
            continue;
        }
        let config = media_config.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        match media.lock().await.start_remote(session_id, share_id, config).await {
            Ok(()) => {
                projected_remote_shares.insert(session_id, share_id);
                remote_retry_after.remove(&session_id);
            }
            Err(error) => {
                remote_retry_after.insert(
                    session_id,
                    (share_id, tokio::time::Instant::now() + Duration::from_secs(1)),
                );
                tracing::warn!("failed to start remote video: {error}");
            }
        }
    }
    remote_retry_after
        .retain(|session_id, (share_id, _)| remote_shares.get(session_id) == Some(share_id));
    *projected_sessions = connected;
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

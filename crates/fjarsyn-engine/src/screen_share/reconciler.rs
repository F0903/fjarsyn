use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use super::{Config, Output, ShareBinding, Update, local, remote, retains_media_session};
use crate::peer_session::{self, RemoteShareState, SessionId};

pub(super) struct Reconciler {
    sessions: peer_session::ServiceHandle,
    output: Output,
    retained_sessions: BTreeSet<SessionId>,
    remote_shares: BTreeMap<SessionId, ShareBinding>,
    remote_retry_after: BTreeMap<SessionId, (ShareBinding, tokio::time::Instant)>,
}

impl Reconciler {
    pub(super) fn new(sessions: peer_session::ServiceHandle, output: Output) -> Self {
        Self {
            sessions,
            output,
            retained_sessions: BTreeSet::new(),
            remote_shares: BTreeMap::new(),
            remote_retry_after: BTreeMap::new(),
        }
    }

    pub(super) async fn reconcile(
        &mut self,
        mut local: Option<&mut local::Controller>,
        remote: &mut remote::Controller,
        snapshot: &peer_session::Sessions,
        config: &Config,
    ) {
        if let Some(local) = local.as_deref_mut() {
            let local_plan = local.reconciliation(snapshot).await;
            if let Some(binding) = local_plan.teardown_pipeline {
                local.teardown(binding).await;
            }
            for binding in local_plan.stop_shares {
                if let Err(error) =
                    self.sessions.stop_screen_share(binding.session_id, binding.share_id).await
                    && !matches!(
                        error,
                        peer_session::Error::SessionNotFound(_)
                            | peer_session::Error::ShareMismatch(_)
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
            .filter(|session| session.phase == peer_session::Phase::Connected)
            .map(|session| session.session_id)
            .collect::<BTreeSet<_>>();

        for session_id in self.retained_sessions.difference(&retained).copied().collect::<Vec<_>>()
        {
            if let Some(local) = local.as_deref_mut() {
                local.stop_session(session_id).await;
            }
            remote.stop_session(session_id).await;
            self.remote_shares.remove(&session_id);
            self.remote_retry_after.remove(&session_id);
            self.output.publish(Update::SessionClosed { session_id });
        }

        let remote_shares = snapshot
            .sessions
            .iter()
            .filter(|session| retains_media_session(session.phase))
            .filter_map(|session| match session.remote_share {
                RemoteShareState::Active { share_id, epoch } => {
                    Some((session.session_id, ShareBinding::new(share_id, epoch)))
                }
                RemoteShareState::Inactive => None,
            })
            .collect::<BTreeMap<_, _>>();

        let replaced_shares = self
            .remote_shares
            .iter()
            .filter_map(|(&session_id, previous_binding)| {
                (remote_shares.get(&session_id) != Some(previous_binding)).then_some(session_id)
            })
            .collect::<Vec<_>>();
        for session_id in replaced_shares {
            remote.stop(session_id).await;
            self.remote_shares.remove(&session_id);
            self.remote_retry_after.remove(&session_id);
        }

        if remote.restart_required() {
            self.remote_shares.clear();
            self.remote_retry_after.clear();
            self.retained_sessions = retained;
            return;
        }

        for &session_id in &connected {
            if let Err(error) = self.ensure_remote_standby(remote, session_id).await {
                tracing::debug!(
                    %session_id,
                    %error,
                    "remote video standby subscription will be retried"
                );
            }
        }

        for (&session_id, &binding) in &remote_shares {
            if self.remote_shares.get(&session_id) == Some(&binding) {
                if remote.is_running(session_id, binding) {
                    continue;
                }
                // A worker failure is terminal for this authenticated share
                // identity. A new ShareId/epoch is required before retrying.
                continue;
            }
            if !connected.contains(&session_id) {
                continue;
            }
            if self.remote_retry_after.get(&session_id).is_some_and(|(failed_binding, retry_at)| {
                *failed_binding == binding && tokio::time::Instant::now() < *retry_at
            }) {
                continue;
            }
            if let Err(error) = self.ensure_remote_standby(remote, session_id).await {
                self.remote_retry_after.insert(
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
            match remote.start(session_id, binding, config.clone()).await {
                Ok(()) => {
                    self.remote_shares.insert(session_id, binding);
                    self.remote_retry_after.remove(&session_id);
                }
                Err(error) => {
                    self.remote_shares.insert(session_id, binding);
                    self.remote_retry_after.remove(&session_id);
                    tracing::warn!(%session_id, %error, "failed to start remote video");
                }
            }
        }
        self.remote_retry_after
            .retain(|session_id, (binding, _)| remote_shares.get(session_id) == Some(binding));
        self.retained_sessions = retained;
    }

    async fn ensure_remote_standby(
        &self,
        remote: &mut remote::Controller,
        session_id: SessionId,
    ) -> Result<(), String> {
        if remote.receiver_ready(session_id) {
            return Ok(());
        }
        let source = self
            .sessions
            .subscribe_remote_video(session_id)
            .await
            .map_err(|error| error.to_string())?;
        remote.install_standby(session_id, source);
        remote
            .receiver_ready(session_id)
            .then_some(())
            .ok_or_else(|| "remote standby source was not retained".into())
    }
}

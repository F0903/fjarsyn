use std::{collections::BTreeMap, sync::Arc};

use super::{CodecDirection, LocalState, RemoteState, ShareBinding, Update};
use crate::{media::frame::Frame, peer_session};

#[derive(Debug, Clone, Default)]
pub struct SessionSnapshot {
    pub local: LocalState,
    pub remote: RemoteState,
    pub local_frame: Option<Arc<Frame>>,
    pub local_frame_binding: Option<ShareBinding>,
    pub remote_frame: Option<Arc<Frame>>,
    pub remote_frame_binding: Option<ShareBinding>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{CodecDirection, LocalState, RemoteState, ShareBinding, Snapshot, Update};
    use crate::{
        identity::PeerId,
        media::{
            Dimensions, PixelFormat,
            frame::{Frame, FrameData},
        },
        peer_session::{
            self, LocalShareState as PeerLocalShareState, RemoteShareState as PeerRemoteShareState,
            SessionId, ShareEpoch, ShareId,
        },
    };

    fn frame() -> Arc<Frame> {
        Arc::new(Frame {
            data: FrameData::Software(bytes::Bytes::new()),
            format: PixelFormat::BGRA8,
            size: Dimensions::new(0, 0),
            duration: None,
        })
    }

    fn peer_snapshot(
        session_id: SessionId,
        local_share: PeerLocalShareState,
        remote_share: PeerRemoteShareState,
    ) -> peer_session::Snapshot {
        peer_snapshot_with_phase(
            session_id,
            local_share,
            remote_share,
            peer_session::Phase::Connected,
        )
    }

    fn peer_snapshot_with_phase(
        session_id: SessionId,
        local_share: PeerLocalShareState,
        remote_share: PeerRemoteShareState,
        phase: peer_session::Phase,
    ) -> peer_session::Snapshot {
        peer_session::Snapshot {
            sessions: Arc::new(vec![peer_session::SessionSnapshot {
                session_id,
                peer_id: PeerId::new("peer-a").unwrap(),
                phase,
                local_share,
                remote_share,
            }]),
        }
    }

    #[test]
    fn codec_quarantine_is_directional_and_rejects_late_frames() {
        let session_id = SessionId::new();
        let binding = ShareBinding::new(ShareId::new(), ShareEpoch::FIRST);
        let shares = peer_snapshot(
            session_id,
            PeerLocalShareState::Active { share_id: binding.share_id(), epoch: binding.epoch() },
            PeerRemoteShareState::Active { share_id: binding.share_id(), epoch: binding.epoch() },
        );
        let mut snapshot = Snapshot::default();
        snapshot.reconcile_shares(&shares);
        snapshot.apply(Update::LocalState { session_id, state: LocalState::Active });
        snapshot.apply(Update::RemoteState { session_id, state: RemoteState::Active });
        snapshot.apply(Update::LocalFrame { session_id, binding, frame: frame() });
        snapshot.apply(Update::RemoteFrame { session_id, binding, frame: frame() });

        snapshot.apply(Update::CodecRestartRequired { direction: CodecDirection::Encoder });

        assert!(snapshot.encoder_restart_required());
        assert!(!snapshot.decoder_restart_required());
        assert!(snapshot.session(session_id).local_frame.is_none());
        assert!(snapshot.session(session_id).remote_frame.is_some());
        snapshot.apply(Update::LocalState { session_id, state: LocalState::Inactive });
        assert!(matches!(snapshot.session(session_id).local, LocalState::Failed(_)));
        snapshot.apply(Update::LocalFrame { session_id, binding, frame: frame() });
        assert!(snapshot.session(session_id).local_frame.is_none());

        snapshot.apply(Update::CodecRestartRequired { direction: CodecDirection::Decoder });
        snapshot.apply(Update::CodecRestartRequired { direction: CodecDirection::Decoder });

        assert!(snapshot.codec_restart_required());
        assert!(snapshot.session(session_id).remote_frame.is_none());
        snapshot.apply(Update::RemoteState { session_id, state: RemoteState::Inactive });
        assert!(matches!(snapshot.session(session_id).remote, RemoteState::Failed(_)));
        snapshot.apply(Update::RemoteFrame { session_id, binding, frame: frame() });
        assert!(snapshot.session(session_id).remote_frame.is_none());
    }

    #[test]
    fn remote_frames_require_the_latest_authenticated_share_epoch() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let binding_a = ShareBinding::new(share_id, ShareEpoch::FIRST);
        let binding_b = ShareBinding::new(
            share_id,
            ShareEpoch::try_from(ShareEpoch::FIRST.value() + 1).unwrap(),
        );
        let active = |binding: ShareBinding| {
            peer_snapshot(
                session_id,
                PeerLocalShareState::Inactive,
                PeerRemoteShareState::Active {
                    share_id: binding.share_id(),
                    epoch: binding.epoch(),
                },
            )
        };
        let stopped = peer_snapshot(
            session_id,
            PeerLocalShareState::Inactive,
            PeerRemoteShareState::Inactive,
        );
        let mut snapshot = Snapshot::default();

        snapshot.reconcile_shares(&active(binding_a));
        snapshot.apply(Update::RemoteFrame { session_id, binding: binding_a, frame: frame() });
        assert!(snapshot.session(session_id).remote_frame.is_some());

        snapshot.reconcile_shares(&active(binding_b));
        assert!(snapshot.session(session_id).remote_frame.is_none());
        snapshot.apply(Update::RemoteFrame { session_id, binding: binding_a, frame: frame() });
        assert!(snapshot.session(session_id).remote_frame.is_none());

        snapshot.apply(Update::RemoteFrame { session_id, binding: binding_b, frame: frame() });
        assert!(snapshot.session(session_id).remote_frame.is_some());

        snapshot.reconcile_shares(&stopped);
        snapshot.apply(Update::RemoteFrame { session_id, binding: binding_b, frame: frame() });
        assert!(snapshot.session(session_id).remote_frame.is_none());
    }

    #[test]
    fn local_preview_frames_require_the_latest_authenticated_share_epoch() {
        let session_id = SessionId::new();
        let share_id = ShareId::new();
        let binding_a = ShareBinding::new(share_id, ShareEpoch::FIRST);
        let binding_b = ShareBinding::new(
            share_id,
            ShareEpoch::try_from(ShareEpoch::FIRST.value() + 1).unwrap(),
        );
        let active = |binding: ShareBinding| {
            peer_snapshot(
                session_id,
                PeerLocalShareState::Active {
                    share_id: binding.share_id(),
                    epoch: binding.epoch(),
                },
                PeerRemoteShareState::Inactive,
            )
        };
        let mut snapshot = Snapshot::default();

        snapshot.reconcile_shares(&active(binding_a));
        snapshot.apply(Update::LocalFrame { session_id, binding: binding_a, frame: frame() });
        assert!(snapshot.session(session_id).local_frame.is_some());

        snapshot.reconcile_shares(&active(binding_b));
        assert!(snapshot.session(session_id).local_frame.is_none());
        snapshot.apply(Update::LocalFrame { session_id, binding: binding_a, frame: frame() });
        assert!(snapshot.session(session_id).local_frame.is_none());

        snapshot.apply(Update::LocalFrame { session_id, binding: binding_b, frame: frame() });
        assert!(snapshot.session(session_id).local_frame.is_some());
    }

    #[test]
    fn reconciliation_prunes_sessions_that_no_longer_retain_media() {
        let session_id = SessionId::new();
        let connected = peer_snapshot(
            session_id,
            PeerLocalShareState::Inactive,
            PeerRemoteShareState::Inactive,
        );
        let disconnecting = peer_snapshot_with_phase(
            session_id,
            PeerLocalShareState::Inactive,
            PeerRemoteShareState::Inactive,
            peer_session::Phase::Disconnecting,
        );
        let mut snapshot = Snapshot::default();
        snapshot.reconcile_shares(&connected);
        snapshot.apply(Update::LocalState { session_id, state: LocalState::Selecting });
        assert!(snapshot.sessions().contains_key(&session_id));

        snapshot.reconcile_shares(&disconnecting);

        assert!(!snapshot.sessions().contains_key(&session_id));
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    sessions: Arc<BTreeMap<peer_session::SessionId, SessionSnapshot>>,
    active_local_shares: BTreeMap<peer_session::SessionId, ShareBinding>,
    active_remote_shares: BTreeMap<peer_session::SessionId, ShareBinding>,
    encoder_restart_required: bool,
    decoder_restart_required: bool,
}

impl Snapshot {
    pub fn sessions(&self) -> &BTreeMap<peer_session::SessionId, SessionSnapshot> {
        &self.sessions
    }

    pub fn session(&self, session_id: peer_session::SessionId) -> SessionSnapshot {
        self.sessions.get(&session_id).cloned().unwrap_or_default()
    }

    pub const fn encoder_restart_required(&self) -> bool {
        self.encoder_restart_required
    }

    pub const fn decoder_restart_required(&self) -> bool {
        self.decoder_restart_required
    }

    pub const fn codec_restart_required(&self) -> bool {
        self.encoder_restart_required || self.decoder_restart_required
    }

    pub(super) fn apply(&mut self, update: Update) {
        let sessions = Arc::make_mut(&mut self.sessions);
        match update {
            Update::LocalState { session_id, state } => {
                if self.encoder_restart_required {
                    return;
                }
                let session = sessions.entry(session_id).or_default();
                session.local = state;
                if matches!(
                    session.local,
                    LocalState::Inactive | LocalState::Stopping | LocalState::Failed(_)
                ) {
                    session.local_frame = None;
                    session.local_frame_binding = None;
                }
            }
            Update::RemoteState { session_id, state } => {
                if self.decoder_restart_required {
                    return;
                }
                let session = sessions.entry(session_id).or_default();
                session.remote = state;
                if matches!(session.remote, RemoteState::Inactive | RemoteState::Failed(_)) {
                    session.remote_frame = None;
                    session.remote_frame_binding = None;
                }
            }
            Update::LocalFrame { session_id, binding, frame } => {
                if self.encoder_restart_required
                    || self.active_local_shares.get(&session_id) != Some(&binding)
                {
                    return;
                }
                let session = sessions.entry(session_id).or_default();
                session.local_frame = Some(frame);
                session.local_frame_binding = Some(binding);
            }
            Update::RemoteFrame { session_id, binding, frame } => {
                if self.decoder_restart_required
                    || self.active_remote_shares.get(&session_id) != Some(&binding)
                {
                    return;
                }
                let session = sessions.entry(session_id).or_default();
                session.remote_frame = Some(frame);
                session.remote_frame_binding = Some(binding);
            }
            Update::CodecRestartRequired { direction } => match direction {
                CodecDirection::Encoder if !self.encoder_restart_required => {
                    self.encoder_restart_required = true;
                    for session in sessions.values_mut() {
                        session.local_frame = None;
                        session.local_frame_binding = None;
                        if !matches!(session.local, LocalState::Inactive) {
                            session.local = LocalState::Failed(
                                "the video encoder is unavailable until Fjarsyn restarts".into(),
                            );
                        }
                    }
                }
                CodecDirection::Decoder if !self.decoder_restart_required => {
                    self.decoder_restart_required = true;
                    for session in sessions.values_mut() {
                        session.remote_frame = None;
                        session.remote_frame_binding = None;
                        if !matches!(session.remote, RemoteState::Inactive) {
                            session.remote = RemoteState::Failed(
                                "the video decoder is unavailable until Fjarsyn restarts".into(),
                            );
                        }
                    }
                }
                CodecDirection::Encoder | CodecDirection::Decoder => {}
            },
            Update::SessionClosed { session_id } => {
                sessions.remove(&session_id);
                self.active_local_shares.remove(&session_id);
                self.active_remote_shares.remove(&session_id);
            }
        }
    }

    /// Removes frames whose exact authenticated share generation is no longer active.
    pub(super) fn reconcile_shares(&mut self, snapshot: &peer_session::Snapshot) -> bool {
        self.active_local_shares = snapshot
            .sessions
            .iter()
            .filter(|session| super::retains_media_session(session.phase))
            .filter_map(|session| match session.local_share {
                peer_session::LocalShareState::Active { share_id, epoch } => {
                    Some((session.session_id, ShareBinding::new(share_id, epoch)))
                }
                peer_session::LocalShareState::Inactive => None,
            })
            .collect();
        self.active_remote_shares = snapshot
            .sessions
            .iter()
            .filter(|session| super::retains_media_session(session.phase))
            .filter_map(|session| match session.remote_share {
                peer_session::RemoteShareState::Active { share_id, epoch } => {
                    Some((session.session_id, ShareBinding::new(share_id, epoch)))
                }
                peer_session::RemoteShareState::Inactive => None,
            })
            .collect();

        let live_sessions = snapshot
            .sessions
            .iter()
            .filter(|session| super::retains_media_session(session.phase))
            .map(|session| session.session_id)
            .collect::<std::collections::BTreeSet<_>>();
        let sessions = Arc::make_mut(&mut self.sessions);
        let previous_len = sessions.len();
        sessions.retain(|session_id, _| live_sessions.contains(session_id));
        let mut changed = sessions.len() != previous_len;
        for (session_id, media) in sessions {
            let active_local = self.active_local_shares.get(session_id).copied();
            if media.local_frame_binding != active_local {
                changed |= media.local_frame.is_some() || media.local_frame_binding.is_some();
                media.local_frame = None;
                media.local_frame_binding = None;
            }

            let active_remote = self.active_remote_shares.get(session_id).copied();
            if media.remote_frame_binding != active_remote {
                changed |= media.remote_frame.is_some() || media.remote_frame_binding.is_some();
                media.remote_frame = None;
                media.remote_frame_binding = None;
            }
        }
        changed
    }
}

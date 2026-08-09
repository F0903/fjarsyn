use std::sync::{Arc, Mutex};

use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};

use super::super::{ActorInstanceId, Command, Control, Handle};
use crate::{
    identity::PeerId,
    peer_session::{
        LocalShareState, Phase, RemoteShareState, SessionId, SessionState, ShareEpoch,
        media::{encoded_video_channel, remote_video_channel},
    },
};

#[tokio::test]
async fn first_remote_video_source_keeps_samples_sent_before_subscription() {
    let session_id = SessionId::new();
    let peer_id = PeerId::new("peer").unwrap();
    let (command_tx, _command_rx) = mpsc::channel(1);
    let (restart_tx, _restart_rx) = mpsc::channel(1);
    let (_snapshot_tx, snapshot_rx) = watch::channel(SessionState {
        session_id,
        peer_id,
        phase: Phase::Connected,
        local_share: LocalShareState::Inactive,
        remote_share: RemoteShareState::Inactive,
    });
    let (encoded_video_tx, _encoded_video_rx) = encoded_video_channel(1);
    let (_active_video_tx, active_video_rx) = watch::channel(None);
    let (remote_video_tx, initial_remote_video_rx) = remote_video_channel(4);
    let (fatal_tx, _fatal_rx) = watch::channel(None);
    let handle = Handle {
        session_id,
        instance_id: ActorInstanceId::new(),
        command_tx,
        restart_tx,
        snapshot_rx,
        encoded_video_tx,
        active_video_rx,
        remote_video_tx: remote_video_tx.clone(),
        initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
        fatal_tx,
    };
    let sample = crate::peer_session::EncodedVideoSample::new(
        bytes::Bytes::from_static(b"initial-idr"),
        std::time::Duration::from_millis(16),
    );

    let epoch = ShareEpoch::FIRST;
    remote_video_tx
        .send(crate::peer_session::media::RemoteVideoSample { epoch, sample: sample.clone() })
        .unwrap();
    let mut source = handle.remote_video_source();

    assert_eq!(
        source.recv_for(epoch).await.unwrap(),
        crate::peer_session::RemoteVideoRead::Sample(sample)
    );
}

#[tokio::test]
async fn fatal_control_bypasses_a_full_session_command_queue() {
    let session_id = SessionId::new();
    let peer_id = PeerId::new("peer").unwrap();
    let (command_tx, _command_rx) = mpsc::channel(1);
    let (restart_tx, _restart_rx) = mpsc::channel(1);
    let (reply, _reply_rx) = oneshot::channel();
    command_tx.send(Command::Accept(reply)).await.unwrap();
    let (_snapshot_tx, snapshot_rx) = watch::channel(SessionState {
        session_id,
        peer_id,
        phase: Phase::Connected,
        local_share: LocalShareState::Inactive,
        remote_share: RemoteShareState::Inactive,
    });
    let (encoded_video_tx, _video_rx) = encoded_video_channel(1);
    let (_active_video_tx, active_video_rx) = watch::channel(None);
    let (remote_video_tx, initial_remote_video_rx) = remote_video_channel(1);
    let (fatal_tx, mut fatal_rx) = watch::channel(None);
    let handle = Handle {
        session_id,
        instance_id: ActorInstanceId::new(),
        command_tx,
        restart_tx,
        snapshot_rx,
        encoded_video_tx,
        active_video_rx,
        remote_video_tx,
        initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
        fatal_tx,
    };

    handle.fail("mandatory sink failed");
    fatal_rx.changed().await.unwrap();
    assert_eq!(*fatal_rx.borrow(), Some(Control::Fail("mandatory sink failed".into())));
}

#[tokio::test]
async fn shutdown_control_bypasses_a_full_session_command_queue() {
    let session_id = SessionId::new();
    let peer_id = PeerId::new("peer").unwrap();
    let (command_tx, _command_rx) = mpsc::channel(1);
    let (restart_tx, _restart_rx) = mpsc::channel(1);
    let (reply, _reply_rx) = oneshot::channel();
    command_tx.send(Command::Accept(reply)).await.unwrap();
    let (_snapshot_tx, snapshot_rx) = watch::channel(SessionState {
        session_id,
        peer_id,
        phase: Phase::Connected,
        local_share: LocalShareState::Inactive,
        remote_share: RemoteShareState::Inactive,
    });
    let (encoded_video_tx, _video_rx) = encoded_video_channel(1);
    let (_active_video_tx, active_video_rx) = watch::channel(None);
    let (remote_video_tx, initial_remote_video_rx) = remote_video_channel(1);
    let (fatal_tx, mut fatal_rx) = watch::channel(None);
    let handle = Handle {
        session_id,
        instance_id: ActorInstanceId::new(),
        command_tx,
        restart_tx,
        snapshot_rx,
        encoded_video_tx,
        active_video_rx,
        remote_video_tx,
        initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
        fatal_tx,
    };
    let deadline = Instant::now() + std::time::Duration::from_secs(1);

    handle.shutdown(deadline);
    fatal_rx.changed().await.unwrap();
    assert_eq!(*fatal_rx.borrow(), Some(Control::Shutdown { deadline }));
}

#[tokio::test]
async fn trust_revocation_bypasses_a_full_session_command_queue() {
    let session_id = SessionId::new();
    let peer_id = PeerId::new("peer").unwrap();
    let (command_tx, _command_rx) = mpsc::channel(1);
    let (restart_tx, _restart_rx) = mpsc::channel(1);
    let (reply, _reply_rx) = oneshot::channel();
    command_tx.send(Command::Accept(reply)).await.unwrap();
    let (_snapshot_tx, snapshot_rx) = watch::channel(SessionState {
        session_id,
        peer_id,
        phase: Phase::Connected,
        local_share: LocalShareState::Inactive,
        remote_share: RemoteShareState::Inactive,
    });
    let (encoded_video_tx, _video_rx) = encoded_video_channel(1);
    let (_active_video_tx, active_video_rx) = watch::channel(None);
    let (remote_video_tx, initial_remote_video_rx) = remote_video_channel(1);
    let (fatal_tx, mut fatal_rx) = watch::channel(None);
    let handle = Handle {
        session_id,
        instance_id: ActorInstanceId::new(),
        command_tx,
        restart_tx,
        snapshot_rx,
        encoded_video_tx,
        active_video_rx,
        remote_video_tx,
        initial_remote_video_rx: Arc::new(Mutex::new(Some(initial_remote_video_rx))),
        fatal_tx,
    };
    let deadline = Instant::now() + std::time::Duration::from_secs(1);

    handle.revoke_trust(deadline);
    fatal_rx.changed().await.unwrap();
    assert_eq!(*fatal_rx.borrow(), Some(Control::TrustRevoked { deadline }));
}

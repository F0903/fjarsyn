use std::time::Duration;

use bytes::Bytes;
use tokio::sync::watch;

use super::{
    EncodedVideoSample, EncodedVideoSink, RemoteVideoRead, RemoteVideoSample, RemoteVideoSource,
    encoded_video_channel, remote_video_channel,
};
use crate::peer_session::{Error, SessionId, ShareEpoch, ShareId};

fn sample(data: &'static [u8]) -> EncodedVideoSample {
    EncodedVideoSample::new(Bytes::from_static(data), Duration::from_millis(16))
}

#[tokio::test]
async fn outbound_sink_immutably_binds_share_and_epoch_at_enqueue() {
    let session_id = SessionId::new();
    let share_id = ShareId::new();
    let epoch = ShareEpoch::from_value(7);
    let (tx, mut rx) = encoded_video_channel(2);
    let (_active_tx, active_rx) = watch::channel(Some((share_id, epoch)));
    let sink = EncodedVideoSink::new(session_id, share_id, epoch, tx, active_rx);

    sink.send(sample(b"frame")).await.unwrap();
    let tagged = rx.recv().await.unwrap();
    assert_eq!(tagged.share_id, share_id);
    assert_eq!(tagged.epoch, epoch);
    assert_eq!(tagged.sample, sample(b"frame"));
}

#[tokio::test]
async fn revoked_sink_cannot_keep_a_new_share_backpressured() {
    let session_id = SessionId::new();
    let share_a = ShareId::new();
    let share_b = ShareId::new();
    let epoch_a = ShareEpoch::FIRST;
    let epoch_b = epoch_a.next().unwrap();
    let (tx, mut rx) = encoded_video_channel(1);
    let (active_tx, active_rx) = watch::channel(Some((share_a, epoch_a)));
    let sink_a = EncodedVideoSink::new(session_id, share_a, epoch_a, tx.clone(), active_rx.clone());

    sink_a.send(sample(b"a-queued")).await.unwrap();
    let blocked_a = tokio::spawn({
        let sink_a = sink_a.clone();
        async move { sink_a.send(sample(b"a-blocked")).await }
    });
    tokio::task::yield_now().await;

    active_tx.send_replace(Some((share_b, epoch_b)));
    assert_eq!(blocked_a.await.unwrap(), Err(Error::MediaClosed));
    assert_eq!(sink_a.try_send(sample(b"a-stale")), Err(Error::MediaClosed));

    assert_eq!(rx.recv().await.unwrap().sample, sample(b"a-queued"));
    let sink_b = EncodedVideoSink::new(session_id, share_b, epoch_b, tx, active_rx);
    sink_b.try_send(sample(b"b-current")).unwrap();
    assert_eq!(rx.recv().await.unwrap().sample, sample(b"b-current"));
}

#[tokio::test]
async fn remote_source_preserves_future_epoch_and_discards_delayed_tail() {
    const A_CURRENT: &[u8] = &[0, 0, 0, 1, 0x65, 0xa1];
    const B_BOOTSTRAP: &[u8] =
        &[0, 0, 0, 1, 0x67, 0xb1, 0, 0, 0, 1, 0x68, 0xb1, 0, 0, 0, 1, 0x65, 0xb1];
    const A_DELAYED_BOOTSTRAP: &[u8] =
        &[0, 0, 0, 1, 0x67, 0xaf, 0, 0, 0, 1, 0x68, 0xaf, 0, 0, 0, 1, 0x65, 0xaf];
    const B_NEXT: &[u8] = &[0, 0, 0, 1, 0x61, 0xb2];
    let epoch_a = ShareEpoch::FIRST;
    let epoch_b = epoch_a.next().unwrap();
    let (tx, rx) = remote_video_channel(8);
    let mut source = RemoteVideoSource::new(rx);

    tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(A_CURRENT) }).unwrap();
    tx.send(RemoteVideoSample { epoch: epoch_b, sample: sample(B_BOOTSTRAP) }).unwrap();
    tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(A_DELAYED_BOOTSTRAP) }).unwrap();
    tx.send(RemoteVideoSample { epoch: epoch_b, sample: sample(B_NEXT) }).unwrap();

    assert_eq!(source.recv_for(epoch_a).await.unwrap(), RemoteVideoRead::Sample(sample(A_CURRENT)));
    assert_eq!(
        source.recv_for(epoch_a).await.unwrap(),
        RemoteVideoRead::EpochAdvanced { next_epoch: epoch_b }
    );
    assert_eq!(
        source.recv_for(epoch_b).await.unwrap(),
        RemoteVideoRead::Sample(sample(B_BOOTSTRAP))
    );
    assert_eq!(source.recv_for(epoch_b).await.unwrap(), RemoteVideoRead::Sample(sample(B_NEXT)));
}

#[tokio::test]
async fn remote_source_recovers_from_lag_without_losing_future_epoch_handoff() {
    let epoch_a = ShareEpoch::FIRST;
    let epoch_b = epoch_a.next().unwrap();
    let (tx, rx) = remote_video_channel(2);
    let mut source = RemoteVideoSource::new(rx);

    tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(b"a-old-1") }).unwrap();
    tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(b"a-old-2") }).unwrap();
    tx.send(RemoteVideoSample { epoch: epoch_a, sample: sample(b"a-retained") }).unwrap();
    tx.send(RemoteVideoSample { epoch: epoch_b, sample: sample(b"b-bootstrap") }).unwrap();

    assert!(matches!(
        source.recv_for(epoch_a).await,
        Err(Error::RemoteVideoLagged { skipped }) if skipped >= 1
    ));
    assert_eq!(
        source.recv_for(epoch_a).await.unwrap(),
        RemoteVideoRead::Sample(sample(b"a-retained"))
    );
    assert_eq!(
        source.recv_for(epoch_a).await.unwrap(),
        RemoteVideoRead::EpochAdvanced { next_epoch: epoch_b }
    );
    assert_eq!(
        source.recv_for(epoch_b).await.unwrap(),
        RemoteVideoRead::Sample(sample(b"b-bootstrap"))
    );
}

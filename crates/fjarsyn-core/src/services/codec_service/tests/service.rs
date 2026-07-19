use std::time::Duration;

use super::{
    super::{CodecWorkerError, ShutdownError},
    support::{
        BlockingGate, EncoderPlan, ReleaseGateOnDrop, ScriptedCodecBackendFactory, encoder_config,
        test_frame, test_service,
    },
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_service_shutdown_joins_an_idle_worker_within_one_deadline() {
    let factory = ScriptedCodecBackendFactory::new(vec![EncoderPlan::Pass], vec![]);
    let (service, handle) = test_service(factory);
    let _encoder = handle.open_encoder(encoder_config()).await.unwrap();

    tokio::time::timeout(Duration::from_millis(200), service.shutdown())
        .await
        .expect("codec service shutdown exceeded its single deadline")
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_call_and_owner_shutdown_share_one_stop_deadline() {
    let gate = BlockingGate::new();
    let _release_gate = ReleaseGateOnDrop(gate.clone());
    let factory = ScriptedCodecBackendFactory::new(vec![EncoderPlan::Block(gate.clone())], vec![]);
    let (service, handle) = test_service(factory);
    let encoder = handle.open_encoder(encoder_config()).await.unwrap();
    encoder.try_send(test_frame()).unwrap();
    gate.wait_until_started().await;

    let started_at = tokio::time::Instant::now();
    service.request_shutdown();
    let (worker_result, service_result) = tokio::join!(encoder.shutdown(), service.shutdown());
    let elapsed = started_at.elapsed();
    gate.release();

    assert!(matches!(worker_result, Err(CodecWorkerError::RestartRequired(_))));
    assert!(matches!(service_result, Err(ShutdownError { remaining_workers: 1 })));
    assert!(
        elapsed < Duration::from_millis(200),
        "shutdown used more than one stop deadline: {elapsed:?}"
    );
}

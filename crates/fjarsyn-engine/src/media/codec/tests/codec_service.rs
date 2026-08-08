use std::time::Duration;

use super::{
    super::{Config, ShutdownError, WorkerError},
    support::{
        BlockingGate, EncoderPlan, ReleaseGateOnDrop, ScriptedBackendFactory, encoder_config,
        test_frame, test_service,
    },
};

#[test]
fn production_deadlines_are_explicit() {
    let config = Config::default();
    assert_eq!(config.call_timeout, Duration::from_secs(10));
    assert_eq!(config.stop_timeout, Duration::from_secs(3));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_service_shutdown_joins_an_idle_worker_within_one_deadline() {
    let factory = ScriptedBackendFactory::new(vec![EncoderPlan::Pass], vec![]);
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
    let factory = ScriptedBackendFactory::new(vec![EncoderPlan::Block(gate.clone())], vec![]);
    let (service, handle) = test_service(factory);
    let encoder = handle.open_encoder(encoder_config()).await.unwrap();
    encoder.try_send(test_frame()).unwrap();
    gate.wait_until_started().await;

    let started_at = tokio::time::Instant::now();
    service.request_shutdown();
    let (worker_result, service_result) = tokio::join!(encoder.shutdown(), service.shutdown());
    let elapsed = started_at.elapsed();
    gate.release();

    assert!(matches!(worker_result, Err(WorkerError::RestartRequired(_))));
    assert!(matches!(service_result, Err(ShutdownError { remaining_workers: 1 })));
    assert!(
        elapsed < Duration::from_millis(200),
        "shutdown used more than one stop deadline: {elapsed:?}"
    );
}

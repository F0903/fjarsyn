use std::time::Duration;

use super::{
    super::{Direction, Operation, Poison, PoisonReason, WorkerError},
    support::{
        BlockingGate, EncoderPlan, ReleaseGateOnDrop, ScriptedBackendFactory, encoder_config,
        test_service, wait_until_reaped,
    },
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn destructor_timeout_poison_is_bounded_and_quarantined() {
    let gate = BlockingGate::new();
    let _release_gate = ReleaseGateOnDrop(gate.clone());
    let factory = ScriptedBackendFactory::new(vec![EncoderPlan::DropBlock(gate.clone())], vec![]);
    let (service, handle) = test_service(factory);
    let encoder = handle.open_encoder(encoder_config()).await.unwrap();

    let started_at = tokio::time::Instant::now();
    let shutdown = tokio::spawn(async move { encoder.shutdown().await });
    gate.wait_until_started().await;
    let result = shutdown.await.unwrap();
    let elapsed = started_at.elapsed();
    gate.release();

    assert!(matches!(
        result,
        Err(WorkerError::RestartRequired(Poison {
            direction: Direction::Encode,
            operation: Operation::Shutdown,
            reason: PoisonReason::DeadlineExceeded,
        }))
    ));
    assert!(elapsed < Duration::from_millis(200));
    wait_until_reaped(&handle).await;
    service.shutdown().await.unwrap();
}

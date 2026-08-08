use std::time::Duration;

use bytes::Bytes;

use super::{
    super::{Direction, DirectionState, Error, Operation, Poison, PoisonReason, WorkerError},
    support::{
        BlockingGate, DecoderPlan, EncoderPlan, ReleaseGateOnDrop, ScriptedBackendFactory,
        decoder_config, encoder_config, test_frame, test_service, wait_until_reaped,
    },
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn returned_creation_and_call_errors_do_not_poison_encoding() {
    let factory = ScriptedBackendFactory::new(
        vec![EncoderPlan::CreateError, EncoderPlan::CallError, EncoderPlan::Pass],
        vec![],
    );
    let (service, handle) = test_service(factory);

    let create_error = handle.open_encoder(encoder_config()).await;
    assert!(matches!(create_error, Err(Error::Codec(_))));
    assert_eq!(handle.snapshot().encode, DirectionState::Available);

    let mut call_error = handle.open_encoder(encoder_config()).await.unwrap();
    call_error.try_send(test_frame()).unwrap();
    assert!(matches!(call_error.recv().await, Some(Err(WorkerError::Codec(_)))));
    assert_eq!(handle.snapshot().encode, DirectionState::Available);
    drop(call_error);

    let healthy = handle.open_encoder(encoder_config()).await.unwrap();
    healthy.shutdown().await.unwrap();
    wait_until_reaped(&handle).await;
    service.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn encode_deadline_poison_is_sticky_and_decode_remains_available() {
    let gate = BlockingGate::new();
    let _release_gate = ReleaseGateOnDrop(gate.clone());
    let factory = ScriptedBackendFactory::new(
        vec![EncoderPlan::Block(gate.clone()), EncoderPlan::Pass],
        vec![DecoderPlan::Pass],
    );
    let (service, handle) = test_service(factory);
    let mut encoder = handle.open_encoder(encoder_config()).await.unwrap();
    let mut decoder = handle.open_decoder(decoder_config()).await.unwrap();

    encoder.try_send(test_frame()).unwrap();
    gate.wait_until_started().await;
    let failure = encoder.recv().await;
    assert!(matches!(
        failure,
        Some(Err(WorkerError::RestartRequired(Poison {
            direction: Direction::Encode,
            operation: Operation::Encode,
            reason: PoisonReason::DeadlineExceeded,
        })))
    ));
    assert!(matches!(handle.snapshot().encode, DirectionState::RestartRequired(_)));
    assert_eq!(handle.snapshot().decode, DirectionState::Available);
    assert!(matches!(handle.open_encoder(encoder_config()).await, Err(Error::RestartRequired(_))));

    decoder.try_send(Bytes::from_static(&[1])).unwrap();
    assert!(matches!(decoder.recv().await, Some(Ok(_))));
    decoder.shutdown().await.unwrap();

    gate.release();
    assert!(encoder.recv().await.is_none(), "late encoder output escaped poison gate");
    drop(encoder);
    wait_until_reaped(&handle).await;
    service.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_decoder_timeout_cancels_siblings_without_poisoning_encode() {
    let gate = BlockingGate::new();
    let _release_gate = ReleaseGateOnDrop(gate.clone());
    let factory = ScriptedBackendFactory::new(
        vec![EncoderPlan::Pass],
        vec![DecoderPlan::Block(gate.clone()), DecoderPlan::Pass],
    );
    let (service, handle) = test_service(factory);
    let mut blocked = handle.open_decoder(decoder_config()).await.unwrap();
    let mut sibling = handle.open_decoder(decoder_config()).await.unwrap();

    blocked.try_send(Bytes::from_static(&[1])).unwrap();
    gate.wait_until_started().await;
    assert!(matches!(blocked.recv().await, Some(Err(WorkerError::RestartRequired(_)))));
    assert!(matches!(sibling.recv().await, Some(Err(WorkerError::RestartRequired(_)))));
    assert_eq!(handle.snapshot().encode, DirectionState::Available);
    assert!(matches!(handle.open_decoder(decoder_config()).await, Err(Error::RestartRequired(_))));

    let encoder = handle.open_encoder(encoder_config()).await.unwrap();
    encoder.shutdown().await.unwrap();
    gate.release();
    drop((blocked, sibling));
    wait_until_reaped(&handle).await;
    service.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constructor_timeout_poison_is_bounded_and_quarantined() {
    let gate = BlockingGate::new();
    let _release_gate = ReleaseGateOnDrop(gate.clone());
    let factory = ScriptedBackendFactory::new(vec![EncoderPlan::CreateBlock(gate.clone())], vec![]);
    let (service, handle) = test_service(factory);

    let started_at = tokio::time::Instant::now();
    let result = handle.open_encoder(encoder_config()).await;
    let elapsed = started_at.elapsed();
    gate.release();

    assert!(matches!(
        result,
        Err(Error::RestartRequired(Poison {
            direction: Direction::Encode,
            operation: Operation::Initialize,
            reason: PoisonReason::DeadlineExceeded,
        }))
    ));
    assert!(elapsed < Duration::from_millis(150));
    wait_until_reaped(&handle).await;
    service.shutdown().await.unwrap();
}

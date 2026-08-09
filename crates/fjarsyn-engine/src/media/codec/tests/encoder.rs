use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::mpsc::error::TrySendError;

use super::{
    super::{EncoderInput, EncoderOutput},
    support::{
        EncoderPlan, ScriptedBackendFactory, encoder_config, test_frame,
        test_frame_without_duration, test_service,
    },
};
use crate::media::frame::Frame;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keyframe_requests_are_coalesced_and_apply_to_the_next_encode_call() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let factory =
        ScriptedBackendFactory::new(vec![EncoderPlan::RecordKeyframeFlags(calls.clone())], vec![]);
    let (service, handle) = test_service(factory);
    let session = handle.open_encoder(encoder_config()).await.unwrap();
    let parts = session.into_parts();
    let input_clone = parts.input.clone();
    let mut output = parts.output;

    parts.input.request_keyframe();
    input_clone.request_keyframe();
    send_when_ready(&parts.input, test_frame_without_duration()).await;
    send_when_ready(&parts.input, test_frame()).await;
    receive_frame(&mut output).await;

    send_when_ready(&parts.input, test_frame()).await;
    receive_frame(&mut output).await;

    input_clone.request_keyframe();
    send_when_ready(&parts.input, test_frame()).await;
    receive_frame(&mut output).await;

    assert_eq!(*calls.lock().unwrap(), [true, false, true]);

    parts.worker.shutdown().await.unwrap();
    service.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_encoded_output_queue_backpressures_without_losing_access_units() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let factory =
        ScriptedBackendFactory::new(vec![EncoderPlan::RecordKeyframeFlags(calls.clone())], vec![]);
    let (service, handle) = test_service(factory);
    let session = handle.open_encoder(encoder_config()).await.unwrap();
    let parts = session.into_parts();
    let mut output = parts.output;

    for _ in 0..6 {
        send_when_ready(&parts.input, test_frame()).await;
    }

    wait_for_call_count(&calls, 4).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(calls.lock().unwrap().len(), 4, "encoder ran past full output queue");

    for _ in 0..6 {
        receive_frame(&mut output).await;
    }
    assert_eq!(calls.lock().unwrap().len(), 6);

    parts.worker.shutdown().await.unwrap();
    service.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_interrupts_an_encoder_blocked_on_full_output_queue() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let factory =
        ScriptedBackendFactory::new(vec![EncoderPlan::RecordKeyframeFlags(calls.clone())], vec![]);
    let (service, handle) = test_service(factory);
    let session = handle.open_encoder(encoder_config()).await.unwrap();
    let parts = session.into_parts();

    for _ in 0..6 {
        send_when_ready(&parts.input, test_frame()).await;
    }
    wait_for_call_count(&calls, 4).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(calls.lock().unwrap().len(), 4, "encoder did not remain backpressured");

    tokio::time::timeout(Duration::from_millis(200), parts.worker.shutdown())
        .await
        .expect("encoder shutdown remained blocked on encoded output")
        .unwrap();

    drop((parts.input, parts.output));
    service.shutdown().await.unwrap();
}

async fn send_when_ready(input: &EncoderInput, mut frame: Arc<Frame>) {
    loop {
        match input.try_send(frame) {
            Ok(()) => return,
            Err(TrySendError::Full(returned)) => {
                frame = returned;
                tokio::task::yield_now().await;
            }
            Err(TrySendError::Closed(_)) => panic!("encoder input closed unexpectedly"),
        }
    }
}

async fn receive_frame(output: &mut EncoderOutput) {
    tokio::time::timeout(Duration::from_secs(1), output.recv())
        .await
        .expect("encoded frame was lost")
        .expect("encoder output ended unexpectedly")
        .expect("encoder failed unexpectedly");
}

async fn wait_for_call_count(calls: &Mutex<Vec<bool>>, expected: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while calls.lock().unwrap().len() < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("encoder did not reach expected call count");
}

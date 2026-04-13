use std::sync::Arc;

use fjarsyn_core::{
    capture_providers::{CaptureFramerate, CaptureProvider, PlatformCaptureProvider},
    media::frame::Frame,
};
use futures::StreamExt;
use tokio::sync::{RwLock, watch};

use super::{EncoderWorker, frame_receiver::LatestFrameReceiverRef, task::AbortOnDropTask};

#[derive(Clone)]
pub(crate) struct CaptureWorker {
    latest_frame_rx: LatestFrameReceiverRef,
    _task: Arc<AbortOnDropTask>,
}

impl std::fmt::Debug for CaptureWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CaptureWorker")
    }
}

pub(crate) struct CaptureWorkerConfig {
    pub(crate) capture: Arc<RwLock<PlatformCaptureProvider>>,
    pub(crate) framerate: CaptureFramerate,
    pub(crate) encoder: EncoderWorker,
}

impl CaptureWorker {
    pub(crate) fn start(config: CaptureWorkerConfig) -> Self {
        let (latest_frame_tx, latest_frame_rx) = watch::channel::<Option<Arc<Frame>>>(None);

        let task = tokio::spawn(async move {
            let mut stream = {
                let mut capture = config.capture.write().await;
                match capture.create_stream(config.framerate) {
                    Ok(stream) => stream,
                    Err(err) => {
                        tracing::error!("Failed to create capture stream: {}", err);
                        return;
                    }
                }
            };

            while let Some(frame) = stream.next().await {
                let frame = Arc::new(frame);

                if let Err(err) = latest_frame_tx.send(Some(frame.clone())) {
                    tracing::debug!(
                        "Stopping capture worker because preview receiver is gone: {}",
                        err
                    );
                    break;
                }

                config.encoder.queue_frame(frame);
            }

            tracing::info!("Capture worker finished.");
        });

        Self {
            latest_frame_rx: LatestFrameReceiverRef::new(latest_frame_rx),
            _task: Arc::new(AbortOnDropTask::new(task)),
        }
    }

    pub(crate) fn frame_receiver(&self) -> LatestFrameReceiverRef {
        self.latest_frame_rx.clone()
    }
}

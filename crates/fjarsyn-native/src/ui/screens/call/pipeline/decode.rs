use std::sync::Arc;

use fjarsyn_core::media::{
    ffmpeg::{FFmpegDecoder, FFmpegTranscodeType},
    frame::Frame,
    pixel_format::PixelFormat,
};
use tokio::sync::watch;

use super::{frame_receiver::LatestFrameReceiverRef, task::AbortOnDropTask};
use crate::ui::subscription::EventReceiverRef;

#[derive(Clone)]
pub(crate) struct DecoderWorker {
    decoded_frame_rx: LatestFrameReceiverRef,
    _task: Arc<AbortOnDropTask>,
}

impl std::fmt::Debug for DecoderWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DecoderWorker")
    }
}

pub(crate) struct DecoderWorkerConfig {
    pub(crate) transcoding_type: FFmpegTranscodeType,
    pub(crate) pixel_format: PixelFormat,
}

impl DecoderWorker {
    pub(crate) fn start(
        config: DecoderWorkerConfig,
        packet_receiver: EventReceiverRef<bytes::Bytes>,
    ) -> Self {
        let (decoded_tx, decoded_rx) = watch::channel::<Option<Arc<Frame>>>(None);

        let task = tokio::spawn(async move {
            let mut decoder = match FFmpegDecoder::new(config.transcoding_type, config.pixel_format)
            {
                Ok(decoder) => decoder,
                Err(e) => {
                    tracing::error!("Failed to create H264 Decoder: {}", e);
                    return;
                }
            };

            loop {
                let Some(packet) = Self::recv_packet(&packet_receiver).await else {
                    let _ = decoded_tx.send(None);
                    break;
                };

                match decoder.decode(&packet) {
                    Ok(Some(frame)) => {
                        if let Err(err) = decoded_tx.send(Some(frame)) {
                            tracing::debug!(
                                "Stopping decoder worker because decoded frame receiver is gone: {}",
                                err
                            );
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::error!("Failed to decode frame: {}", e);
                    }
                }
            }

            tracing::info!("Decoder worker finished.");
        });

        Self {
            decoded_frame_rx: LatestFrameReceiverRef::new(decoded_rx),
            _task: Arc::new(AbortOnDropTask::new(task)),
        }
    }

    pub(crate) fn frame_receiver(&self) -> LatestFrameReceiverRef {
        self.decoded_frame_rx.clone()
    }

    async fn recv_packet(packet_receiver: &EventReceiverRef<bytes::Bytes>) -> Option<bytes::Bytes> {
        {
            let mut lock = packet_receiver.0.lock().await;
            lock.recv().await
        }
    }
}

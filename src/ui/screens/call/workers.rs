use std::sync::{Arc, Mutex as StdMutex};

use futures::StreamExt;
use tokio::{
    sync::{Mutex, RwLock, watch},
    task::JoinHandle,
};

use crate::{
    capture_providers::{CaptureFramerate, CaptureProvider, PlatformCaptureProvider},
    media::{
        TargetResolution,
        ffmpeg::{FFmpegDecoder, FFmpegEncoder, FFmpegTranscodeType, HWAccelType},
        frame::Frame,
        pixel_format::PixelFormat,
    },
    networking::webrtc::WebRTC,
    ui::subscription::EventReceiverRef,
};

#[derive(Clone)]
pub(crate) struct LatestFrameReceiverRef(pub Arc<Mutex<watch::Receiver<Option<Arc<Frame>>>>>);

impl std::fmt::Debug for LatestFrameReceiverRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LatestFrameReceiverRef")
    }
}

impl std::hash::Hash for LatestFrameReceiverRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as *const ()).hash(state);
    }
}

impl PartialEq for LatestFrameReceiverRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for LatestFrameReceiverRef {}

#[derive(Clone)]
pub(crate) struct CaptureWorker {
    latest_frame_rx: LatestFrameReceiverRef,
    _inner: Arc<CaptureWorkerInner>,
}

struct CaptureWorkerInner {
    task: StdMutex<Option<JoinHandle<()>>>,
}

impl Drop for CaptureWorkerInner {
    fn drop(&mut self) {
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
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
            latest_frame_rx: LatestFrameReceiverRef(Arc::new(Mutex::new(latest_frame_rx))),
            _inner: Arc::new(CaptureWorkerInner { task: StdMutex::new(Some(task)) }),
        }
    }

    pub(crate) fn frame_receiver(&self) -> LatestFrameReceiverRef {
        self.latest_frame_rx.clone()
    }
}

#[derive(Clone)]
pub(crate) struct DecoderWorker {
    decoded_frame_rx: LatestFrameReceiverRef,
    _inner: Arc<DecoderWorkerInner>,
}

struct DecoderWorkerInner {
    task: StdMutex<Option<JoinHandle<()>>>,
}

impl Drop for DecoderWorkerInner {
    fn drop(&mut self) {
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
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
            decoded_frame_rx: LatestFrameReceiverRef(Arc::new(Mutex::new(decoded_rx))),
            _inner: Arc::new(DecoderWorkerInner { task: StdMutex::new(Some(task)) }),
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

#[derive(Clone)]
pub(crate) struct EncoderWorker {
    frame_tx: watch::Sender<Option<Arc<Frame>>>,
}

impl std::fmt::Debug for EncoderWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EncoderWorker")
    }
}

pub(crate) struct EncoderWorkerConfig {
    pub(crate) capture: Arc<RwLock<PlatformCaptureProvider>>,
    pub(crate) webrtc: Arc<WebRTC>,
    pub(crate) target_bitrate: u32,
    pub(crate) target_fps_hz: f32,
    pub(crate) target_resolution: TargetResolution,
    pub(crate) input_format: PixelFormat,
    pub(crate) transcoding_type: FFmpegTranscodeType,
}

impl EncoderWorker {
    pub(crate) fn start(config: EncoderWorkerConfig) -> Self {
        let (frame_tx, mut rx) = watch::channel::<Option<Arc<Frame>>>(None);

        tokio::spawn(async move {
            let device_handle =
                if config.transcoding_type.get_encoder_info().hw_accel == HWAccelType::D3D11VA {
                    config.capture.read().await.raw_device_handle()
                } else {
                    None
                };

            let mut encoder = match FFmpegEncoder::new(
                config.target_bitrate,
                config.target_fps_hz,
                config.target_resolution,
                config.input_format,
                device_handle,
                config.transcoding_type,
            ) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!("Failed to create encoder: {}", e);
                    return;
                }
            };

            loop {
                if rx.changed().await.is_err() {
                    break;
                }

                let Some(frame) = rx.borrow_and_update().clone() else {
                    continue;
                };

                match encoder.encode(&frame, config.transcoding_type, frame.size.x, frame.size.y) {
                    Ok(nal_units) => {
                        let frame_duration = match frame.duration {
                            Some(duration) => duration,
                            None => {
                                tracing::error!("Frame duration is None!");
                                continue;
                            }
                        };

                        for nal in nal_units {
                            if let Err(e) = config.webrtc.write_sample(nal, frame_duration).await {
                                tracing::error!("WebRTC write failed: {}", e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Encoding failed: {}", e);
                    }
                }
            }

            tracing::info!("Encoder worker finished.");
        });

        Self { frame_tx }
    }

    pub(crate) fn queue_frame(&self, frame: Arc<Frame>) {
        self.frame_tx.send_replace(Some(frame));
    }
}

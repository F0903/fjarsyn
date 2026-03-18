use std::sync::Arc;

use iced::Task;
use tokio::sync::mpsc;

use super::{CallMessage, CallScreen, DecodedFrameReceiverRef};
use crate::{
    capture_providers::{CaptureProvider, user_pick_platform_capture_item},
    media::{
        ffmpeg::{FFmpegDecoder, FFmpegEncoder},
        frame::Frame,
    },
    services::call_service::CallEvent,
    ui::{
        app::AppState,
        message::{CallServiceMessage, Message, NavigationMessage, Route, ScreenMessage},
    },
};

impl CallScreen {
    pub(crate) fn handle_update(&mut self, ctx: &mut AppState, message: Message) -> Task<Message> {
        match message {
            Message::CallService(CallServiceMessage::PacketReceived(packet)) => {
                self.handle_packet_received(ctx, packet)
            }

            Message::Screen(ScreenMessage::Call(msg)) => match msg {
                CallMessage::DecodedFrameReady(frame) => {
                    self.remote_frame = Some(frame);
                    Task::none()
                }

                CallMessage::ToggleLocalPreview => {
                    self.show_local_preview = !self.show_local_preview;
                    Task::none()
                }

                CallMessage::EndCall => {
                    self.frame_sender = None;
                    self.decoder_sender = None;
                    self.decoded_frame_rx = None;
                    self.local_frame = None;
                    self.remote_frame = None;
                    self.pending_capture_start = false;
                    self.show_local_preview = false;
                    let stop_capture_task = self
                        .capture
                        .clone()
                        .map(|capture_arc| {
                            Task::future(async move {
                                let mut capture = capture_arc.write().await;
                                if let Err(e) = capture.stop_capture() {
                                    tracing::error!("Failed to stop capture: {}", e);
                                }
                                Message::Screen(ScreenMessage::Call(CallMessage::CaptureStopped))
                            })
                        })
                        .unwrap_or_else(Task::none);

                    let disconnect_task = if let Some(service) = &ctx.services.call_service {
                        let service_clone = service.clone();
                        Task::future(async move {
                            if let Err(e) = service_clone.end().await {
                                tracing::error!("Failed to end call: {}", e);
                            }
                            Message::NoOp
                        })
                    } else {
                        Task::none()
                    };

                    Task::batch(vec![
                        stop_capture_task,
                        disconnect_task,
                        Task::done(Message::Navigation(NavigationMessage::Navigate(Route::Home))),
                    ])
                }
                CallMessage::StartCapture => {
                    if self.capture.is_none() {
                        self.pending_capture_start = true;

                        if ctx.media.capture_initializing {
                            return Task::none();
                        }

                        ctx.media.capture_initializing = true;
                        ctx.notify_info("Initializing screen capture...");
                        return crate::ui::app::Fjarsyn::init_capture_task(&ctx.config);
                    }

                    let window_handle = match ctx.ui.main_window.as_ref().and_then(|w| w.raw_id) {
                        Some(handle) => handle,
                        None => {
                            ctx.notify_error(
                                "Screen capture picker is unavailable without an active window.",
                            );
                            return Task::none();
                        }
                    };

                    match user_pick_platform_capture_item(window_handle) {
                        Ok(future) => Task::future(async move {
                            match future.await {
                                Ok(item) => Message::Screen(ScreenMessage::Call(
                                    CallMessage::PlatformUserPickedCaptureItem(Ok(item)),
                                )),
                                Err(e) => Message::Screen(ScreenMessage::Call(
                                    CallMessage::PlatformUserPickedCaptureItem(Err(e.to_string())),
                                )),
                            }
                        }),
                        Err(err) => {
                            ctx.notify_error(format!("Failed to open capture picker: {}", err));
                            Task::none()
                        }
                    }
                }

                CallMessage::PlatformUserPickedCaptureItem(capture_item_result) => {
                    let capture_item = match capture_item_result {
                        Ok(item) => item,
                        Err(err) => {
                            ctx.notify_error(format!("Failed to pick capture item: {}", err));
                            return Task::none();
                        }
                    };
                    Task::done(Message::Screen(ScreenMessage::Call(CallMessage::TryStartCapture(
                        capture_item,
                    ))))
                }

                CallMessage::TryStartCapture(capture_item) => match self.capture.as_ref() {
                    None => {
                        ctx.notify_error("Screen capture is not available.");
                        Task::none()
                    }
                    Some(capture) => match capture.try_write() {
                        Ok(mut capture) => {
                            if let Err(err) = capture.set_capture_item(capture_item.clone()) {
                                ctx.notify_error(format!(
                                    "Failed to select capture source: {}",
                                    err
                                ));
                                return Task::none();
                            }

                            if let Err(err) = capture.start_capture() {
                                ctx.notify_error(format!("Failed to start capture: {}", err));
                                return Task::none();
                            }

                            Task::done(Message::Screen(ScreenMessage::Call(
                                CallMessage::CaptureStarted,
                            )))
                        }
                        Err(_) => {
                            let capture_arc = self.capture.clone();
                            Task::future(async move {
                                if let Some(capture_arc) = capture_arc {
                                    let _lock = capture_arc.write().await;
                                }
                            })
                            .map(move |_| {
                                Message::Screen(ScreenMessage::Call(CallMessage::TryStartCapture(
                                    capture_item.clone(),
                                )))
                            })
                        }
                    },
                },

                CallMessage::CaptureStarted => {
                    self.capture_subscription_revision =
                        self.capture_subscription_revision.wrapping_add(1);
                    Task::none()
                }

                CallMessage::StopCapture => {
                    Task::done(Message::Screen(ScreenMessage::Call(CallMessage::TryStopCapture)))
                }

                CallMessage::TryStopCapture => match self.capture.as_ref() {
                    None => Task::done(Message::Screen(ScreenMessage::Call(
                        CallMessage::CaptureStopped,
                    ))),
                    Some(capture) => match capture.try_write() {
                        Ok(mut capture) => {
                            if let Err(err) = capture.stop_capture() {
                                ctx.notify_error(format!("Failed to stop capture: {}", err));
                            }
                            Task::done(Message::Screen(ScreenMessage::Call(
                                CallMessage::CaptureStopped,
                            )))
                        }
                        Err(_) => {
                            tracing::debug!(
                                "Failed to acquire capture lock. Trying again with waiter..."
                            );
                            let capture_arc = self.capture.clone();
                            Task::future(async move {
                                if let Some(capture_arc) = capture_arc {
                                    let _lock = capture_arc.write().await;
                                }
                                Message::Screen(ScreenMessage::Call(CallMessage::TryStopCapture))
                            })
                        }
                    },
                },

                CallMessage::CaptureStopped => {
                    self.frame_sender = None;
                    self.local_frame = None;
                    self.decoder_sender = None;
                    self.decoded_frame_rx = None;
                    self.remote_frame = None;
                    self.pending_capture_start = false;
                    self.show_local_preview = false;
                    Task::none()
                }

                CallMessage::FrameCaptured(frame) => self.handle_frame_captured(ctx, frame),
            },

            // End the call if the peer disconnects
            Message::CallService(CallServiceMessage::CallEvent(CallEvent::CallEnded)) => {
                Task::done(Message::Screen(ScreenMessage::Call(CallMessage::EndCall)))
            }

            _ => Task::none(),
        }
    }

    fn handle_packet_received(&mut self, ctx: &AppState, packet: bytes::Bytes) -> Task<Message> {
        if self.decoder_sender.is_none() {
            let (packet_tx, mut packet_rx) = mpsc::unbounded_channel::<bytes::Bytes>();
            let (decoded_tx, decoded_rx) = mpsc::unbounded_channel::<Arc<Frame>>();
            self.decoder_sender = Some(packet_tx.clone());
            self.decoded_frame_rx =
                Some(DecodedFrameReceiverRef(Arc::new(tokio::sync::Mutex::new(decoded_rx))));

            let transcoding_type = ctx.config.transcoding_type;
            let pixel_format = ctx.config.pixel_format;

            tokio::spawn(async move {
                let mut decoder = match FFmpegDecoder::new(transcoding_type, pixel_format) {
                    Ok(decoder) => decoder,
                    Err(e) => {
                        tracing::error!("Failed to create H264 Decoder: {}", e);
                        return;
                    }
                };

                while let Some(packet) = packet_rx.recv().await {
                    match decoder.decode(&packet) {
                        Ok(Some(frame)) => {
                            if decoded_tx.send(frame).is_err() {
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
        }

        if let Some(sender) = &self.decoder_sender
            && let Err(err) = sender.send(packet)
        {
            tracing::warn!("Failed to queue packet for decoder: {}", err);
            self.decoder_sender = None;
            self.decoded_frame_rx = None;
        }

        Task::none()
    }

    fn handle_frame_captured(&mut self, ctx: &mut AppState, frame: Arc<Frame>) -> Task<Message> {
        self.local_frame = Some(frame.clone());

        if self.frame_sender.is_none() {
            let Some(service) = &ctx.services.call_service else {
                tracing::error!("CallService is not initialized yet");
                return Task::none();
            };
            let Some(capture) = self.capture.clone() else {
                ctx.notify_error("Screen capture is not available.");
                return Task::none();
            };

            let (tx, mut rx) = mpsc::channel::<Arc<Frame>>(10);
            self.frame_sender = Some(tx.clone());

            let webrtc = service.webrtc().clone();
            let target_fps_hz = ctx.config.target_framerate.to_hz();
            let target_resolution = ctx.config.target_resolution;
            let target_bitrate = ctx.config.target_bitrate;
            let transcoding_type = ctx.config.transcoding_type;
            let input_format = ctx.config.pixel_format;

            tracing::debug!(
                "Starting encoder thread. target_fps_hz: {}, target_resolution: {:?}, target_bitrate: {}",
                target_fps_hz,
                target_resolution,
                target_bitrate
            );
            tokio::spawn(async move {
                let device_handle = if transcoding_type.get_encoder_info().hw_accel
                    == crate::media::ffmpeg::HWAccelType::D3D11VA
                {
                    capture.read().await.raw_device_handle()
                } else {
                    None
                };

                let mut encoder = match FFmpegEncoder::new(
                    target_bitrate,
                    target_fps_hz,
                    target_resolution,
                    input_format,
                    device_handle,
                    transcoding_type,
                ) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::error!("Failed to create encoder: {}", e);
                        return;
                    }
                };

                while let Some(frame) = rx.recv().await {
                    match encoder.encode(&frame, transcoding_type, frame.size.x, frame.size.y) {
                        Ok(nal_units) => {
                            let frame_duration = match frame.duration {
                                Some(duration) => duration,
                                None => {
                                    tracing::error!("Frame duration is None!");
                                    continue;
                                }
                            };
                            for nal in nal_units {
                                if let Err(e) = webrtc.write_sample(nal, frame_duration).await {
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
                tracing::info!("Encoder thread finished.");
            });
        }

        if let Some(tx) = &self.frame_sender {
            match tx.try_send(frame) {
                Ok(_) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    tracing::debug!("Encoder queue full, dropping frame");
                }
                Err(e) => {
                    tracing::warn!("Failed to send frame to encoder: {}", e);
                }
            }
        }

        Task::none()
    }
}

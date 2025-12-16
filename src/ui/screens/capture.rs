use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use bytes::Bytes;
use iced::{
    Element, Length, Subscription, Task,
    widget::{button, container, pick_list, row, text},
};
use tokio::sync::{Mutex, mpsc};

use super::Screen;
use crate::{
    capture_providers::{
        CaptureProvider, PlatformCaptureProvider, PlatformCaptureStream,
        shared::{CaptureFramerate, Frame, Vector2},
        user_pick_platform_capture_item,
    },
    media::h264::{H264Decoder, H264Encoder},
    ui::{app::Message, frame_viewer, state::State},
};

#[derive(Debug, Clone)]
struct FrameReceiverSubData {
    capture: Arc<Mutex<PlatformCaptureProvider>>,
    framerate: CaptureFramerate,
    stream_name: &'static str,
}

impl Hash for FrameReceiverSubData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.stream_name.hash(state);
    }
}

#[derive(Debug, Clone)]
pub struct CaptureScreen {
    capture: Arc<Mutex<PlatformCaptureProvider>>,
}

impl CaptureScreen {
    pub fn new(capture: Arc<Mutex<PlatformCaptureProvider>>) -> Self {
        Self { capture }
    }

    fn create_frame_receiver_subscription(data: &FrameReceiverSubData) -> PlatformCaptureStream {
        tracing::info!("Creating frame receiver sub with framerate: {}", data.framerate);

        data.capture
            .blocking_lock()
            .create_stream(data.framerate)
            .expect("Failed to create stream!")
    }
}

impl Screen for CaptureScreen {
    fn subscription(&self, state: &State) -> Subscription<Message> {
        let mut subscriptions = vec![];

        if state.capturing {
            subscriptions.push(
                Subscription::<Frame>::run_with(
                    FrameReceiverSubData {
                        capture: self.capture.clone(),
                        framerate: state.capture_frame_rate,
                        stream_name: "frame-receiver",
                    },
                    Self::create_frame_receiver_subscription,
                )
                .map(|f| Message::FrameCaptured(Arc::new(f))),
            );
        }

        Subscription::batch(subscriptions)
    }

    fn update(&self, state: &mut State, message: Message) -> Task<Message> {
        match message {
            Message::RemoteFrameReceived(bytes) => {
                if state.decoder.is_none() {
                    match H264Decoder::new() {
                        Ok(decoder) => state.decoder = Some(Arc::new(Mutex::new(decoder))),
                        Err(e) => {
                            tracing::error!("Failed to create H264 Decoder: {}", e);
                            return Task::none();
                        }
                    }
                }

                if let Some(decoder) = &state.decoder {
                    let decoder = decoder.clone();
                    Task::future(async move {
                        let mut lock = decoder.lock().await;
                        match lock.decode(&bytes) {
                            Ok(Some((frame_data, (w, h)))) => Message::DecodedFrameReady(
                                Bytes::from(frame_data),
                                Vector2::new(w as i32, h as i32),
                            ),
                            Ok(None) => Message::NoOp,
                            Err(e) => {
                                tracing::error!("Failed to decode frame: {}", e);
                                Message::NoOp
                            }
                        }
                    })
                } else {
                    Task::none()
                }
            }

            Message::DecodedFrameReady(frame_data, size) => {
                state.frame_data = Some(frame_data);
                state.frame_dimensions = size;
                Task::none()
            }

            Message::StartCapture => {
                let window_handle = match state.main_window_handle {
                    Some(handle) => handle,
                    None => {
                        return Task::done(Message::Error("No active window handle".to_string()));
                    }
                };

                match user_pick_platform_capture_item(window_handle) {
                    Ok(future) => Task::future(async move {
                        match future.await {
                            Ok(item) => Message::PlatformUserPickedCaptureItem(Ok(item)),
                            Err(e) => Message::PlatformUserPickedCaptureItem(Err(e.to_string())),
                        }
                    }),
                    Err(err) => {
                        Task::done(Message::Error(format!("Failed to pick capture item: {}", err)))
                    }
                }
            }

            Message::PlatformUserPickedCaptureItem(capture_item_result) => {
                let capture_item = match capture_item_result {
                    Ok(item) => item,
                    Err(err) => {
                        return Task::done(Message::Error(format!(
                            "Failed to pick capture item: {}",
                            err
                        )));
                    }
                };

                Task::done(Message::TryStartCapture(capture_item))
            }

            Message::TryStartCapture(capture_item) => match self.capture.try_lock() {
                Ok(mut capture) => {
                    if let Err(err) = capture.set_capture_item(capture_item.clone()) {
                        return Task::done(Message::Error(format!(
                            "Failed to set capture item: {}",
                            err
                        )));
                    }

                    if let Err(err) = capture.start_capture() {
                        return Task::done(Message::Error(format!(
                            "Failed to start capture: {}",
                            err
                        )));
                    }

                    Task::done(Message::CaptureStarted)
                }
                Err(_) => {
                    let capture_arc = self.capture.clone();
                    Task::future(async move {
                        let _lock = capture_arc.lock().await;
                    })
                    .map(move |_| Message::TryStartCapture(capture_item.clone()))
                }
            },

            Message::CaptureStarted => {
                state.capturing = true;
                Task::none()
            }

            Message::StopCapture => Task::done(Message::TryStopCapture),

            Message::TryStopCapture => match self.capture.try_lock() {
                Ok(mut capture) => {
                    if let Err(err) = capture.stop_capture() {
                        tracing::error!("Failed to stop capture: {}", err);
                    }

                    Task::done(Message::CaptureStopped)
                }

                Err(_) => {
                    let capture_arc = self.capture.clone();
                    Task::future(async move {
                        let _lock = capture_arc.lock().await;
                    })
                    .map(move |_| Message::TryStopCapture)
                }
            },

            Message::CaptureStopped => {
                state.capturing = false;
                state.frame_sender = None;
                Task::none()
            }

            Message::FrameRateSelected(rate) => {
                state.capture_frame_rate = rate;
                Task::none()
            }

            Message::FrameCaptured(frame) => {
                state.frame_format = frame.format.clone();
                state.frame_dimensions = frame.size;
                state.frame_data = Some(Bytes::copy_from_slice(&frame.data));

                // Since we shouldn't block the main thread for too long, we spawn a task to handle the frame
                if state.frame_sender.is_none() {
                    let Some(Ok(webrtc)) = &state.webrtc else {
                        tracing::error!("WebRTC is not initialized yet");
                        return Task::none();
                    };

                    let (tx, mut rx) = mpsc::channel::<Arc<Frame>>(2);
                    state.frame_sender = Some(tx.clone());

                    let webrtc = webrtc.clone();
                    let _width = frame.size.x as u32;
                    let _height = frame.size.y as u32;
                    let target_fps = state.capture_frame_rate.to_hz();

                    tokio::spawn(async move {
                        let mut encoder = match H264Encoder::new(2_000_000, target_fps) {
                            Ok(enc) => enc,
                            Err(e) => {
                                tracing::error!(
                                    "Failed to create encoder in background task: {}",
                                    e
                                );
                                return;
                            }
                        };

                        while let Some(frame) = rx.recv().await {
                            match encoder.encode(&frame.data, frame.size.x, frame.size.y) {
                                Ok(nal_units) => {
                                    let frame_duration = frame.duration;
                                    for nal in nal_units {
                                        if let Err(e) = webrtc
                                            .write_frame(Bytes::from(nal), frame_duration)
                                            .await
                                        {
                                            tracing::error!("WebRTC write failed: {}", e);
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

                if let Some(tx) = &state.frame_sender {
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

            Message::Error(err) => {
                if !err.is_empty() {
                    tracing::error!("Error: {}", err);
                }
                Task::none()
            }

            _ => Task::none(),
        }
    }

    fn view(&self, state: &State) -> Element<'_, Message> {
        let control_row: Element<Message> = container(
            row([
                if state.capturing {
                    button(text(state.capture_frame_rate.to_string()))
                        .style(iced::widget::button::secondary)
                        .into()
                } else {
                    pick_list(
                        CaptureFramerate::ALL,
                        Some(state.capture_frame_rate),
                        Message::FrameRateSelected,
                    )
                    .into()
                },
                button("Start Capture")
                    .on_press_maybe(if state.capturing {
                        None
                    } else {
                        Some(Message::StartCapture)
                    })
                    .into(),
                button("Stop Capture")
                    .on_press_maybe(if state.capturing { Some(Message::StopCapture) } else { None })
                    .into(),
            ])
            .spacing(10),
        )
        .padding(10)
        .center_x(Length::Fill)
        .into();

        let screen_share_preview = match &state.frame_data {
            Some(frame_data) => container(frame_viewer::frame_viewer(
                frame_data.clone(),
                state.frame_dimensions.x as u32,
                state.frame_dimensions.y as u32,
            ))
            .center(Length::Fill)
            .into(),
            None => container(text("No preview available.")).center(Length::Fill).into(),
        };

        iced::widget::column([control_row, screen_share_preview]).into()
    }
}

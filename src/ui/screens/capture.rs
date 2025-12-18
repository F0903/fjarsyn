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
    ui::{frame_viewer, message::Message, state::AppContext},
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

#[derive(Clone)]

pub struct CaptureScreen {
    capture: Arc<Mutex<PlatformCaptureProvider>>,

    pub capturing: bool,
    pub capture_frame_rate: CaptureFramerate,

    pub frame_data: Option<Bytes>,
    pub frame_dimensions: Vector2<i32>,
    pub frame_format: crate::capture_providers::shared::PixelFormat,
    pub frame_sender: Option<mpsc::Sender<Arc<Frame>>>,

    pub decoder: Option<Arc<Mutex<H264Decoder>>>,
}

impl std::fmt::Debug for CaptureScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureScreen")
            .field("capturing", &self.capturing)
            .field("capture_frame_rate", &self.capture_frame_rate)
            .finish()
    }
}

impl CaptureScreen {
    pub fn new(capture: Arc<Mutex<PlatformCaptureProvider>>) -> Self {
        Self {
            capture,
            capturing: false,
            capture_frame_rate: CaptureFramerate::FPS60,

            frame_data: None,
            frame_dimensions: Vector2::new(0, 0),
            frame_format: crate::capture_providers::shared::PixelFormat::BGRA8,
            frame_sender: None,

            decoder: None,
        }
    }

    fn create_frame_receiver_subscription(data: &FrameReceiverSubData) -> PlatformCaptureStream {
        tracing::info!("Creating frame receiver sub with framerate: {}", data.framerate);

        data.capture
            .blocking_lock()
            .create_stream(data.framerate)
            .expect("Failed to create stream!")
    }
}

#[derive(Debug, Clone)]
pub enum CaptureMessage {
    StartCapture,
    CaptureStarted,
    StopCapture,
    CaptureStopped,
    TryStartCapture(crate::capture_providers::PlatformCaptureItem),
    TryStopCapture,
    PlatformUserPickedCaptureItem(Result<crate::capture_providers::PlatformCaptureItem, String>),
    FrameCaptured(Arc<Frame>),
    FrameRateSelected(CaptureFramerate),
    DecodedFrameReady(Bytes, Vector2<i32>),
}

impl Screen for CaptureScreen {
    fn subscription(&self, _ctx: &AppContext) -> Subscription<Message> {
        let mut subscriptions = vec![];

        if self.capturing {
            subscriptions.push(
                Subscription::<Frame>::run_with(
                    FrameReceiverSubData {
                        capture: self.capture.clone(),

                        framerate: self.capture_frame_rate,
                        stream_name: "frame-receiver",
                    },
                    Self::create_frame_receiver_subscription,
                )
                .map(|f| Message::Capture(CaptureMessage::FrameCaptured(Arc::new(f)))),
            );
        }

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::RemoteFrameReceived(bytes) => {
                if self.decoder.is_none() {
                    match H264Decoder::new() {
                        Ok(decoder) => self.decoder = Some(Arc::new(Mutex::new(decoder))),

                        Err(e) => {
                            tracing::error!("Failed to create H264 Decoder: {}", e);

                            return Task::none();
                        }
                    }
                }

                if let Some(decoder) = &self.decoder {
                    let decoder = decoder.clone();

                    Task::future(async move {
                        let mut lock = decoder.lock().await;

                        match lock.decode(&bytes) {
                            Ok(Some((frame_data, (w, h)))) => {
                                Message::Capture(CaptureMessage::DecodedFrameReady(
                                    Bytes::from(frame_data),
                                    Vector2::new(w as i32, h as i32),
                                ))
                            }

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

            Message::Capture(msg) => match msg {
                CaptureMessage::DecodedFrameReady(frame_data, size) => {
                    self.frame_data = Some(frame_data);
                    self.frame_dimensions = size;
                    Task::none()
                }

                CaptureMessage::StartCapture => {
                    let window_handle = match ctx.main_window_handle {
                        Some(handle) => handle,
                        None => {
                            return Task::done(Message::Error(
                                "No active window handle".to_string(),
                            ));
                        }
                    };

                    match user_pick_platform_capture_item(window_handle) {
                        Ok(future) => Task::future(async move {
                            match future.await {
                                Ok(item) => Message::Capture(
                                    CaptureMessage::PlatformUserPickedCaptureItem(Ok(item)),
                                ),
                                Err(e) => {
                                    Message::Capture(CaptureMessage::PlatformUserPickedCaptureItem(
                                        Err(e.to_string()),
                                    ))
                                }
                            }
                        }),
                        Err(err) => Task::done(Message::Error(format!(
                            "Failed to pick capture item: {}",
                            err
                        ))),
                    }
                }

                CaptureMessage::PlatformUserPickedCaptureItem(capture_item_result) => {
                    let capture_item = match capture_item_result {
                        Ok(item) => item,
                        Err(err) => {
                            return Task::done(Message::Error(format!(
                                "Failed to pick capture item: {}",
                                err
                            )));
                        }
                    };
                    Task::done(Message::Capture(CaptureMessage::TryStartCapture(capture_item)))
                }

                CaptureMessage::TryStartCapture(capture_item) => match self.capture.try_lock() {
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

                        Task::done(Message::Capture(CaptureMessage::CaptureStarted))
                    }
                    Err(_) => {
                        let capture_arc = self.capture.clone();
                        Task::future(async move {
                            let _lock = capture_arc.lock().await;
                        })
                        .map(move |_| {
                            Message::Capture(CaptureMessage::TryStartCapture(capture_item.clone()))
                        })
                    }
                },

                CaptureMessage::CaptureStarted => {
                    self.capturing = true;
                    Task::none()
                }

                CaptureMessage::StopCapture => {
                    Task::done(Message::Capture(CaptureMessage::TryStopCapture))
                }

                CaptureMessage::TryStopCapture => match self.capture.try_lock() {
                    Ok(mut capture) => {
                        if let Err(err) = capture.stop_capture() {
                            tracing::error!("Failed to stop capture: {}", err);
                        }
                        Task::done(Message::Capture(CaptureMessage::CaptureStopped))
                    }
                    Err(_) => {
                        let capture_arc = self.capture.clone();
                        Task::future(async move {
                            let _lock = capture_arc.lock().await;
                        })
                        .map(move |_| Message::Capture(CaptureMessage::TryStopCapture))
                    }
                },

                CaptureMessage::CaptureStopped => {
                    self.capturing = false;
                    self.frame_sender = None;
                    Task::none()
                }

                CaptureMessage::FrameRateSelected(rate) => {
                    self.capture_frame_rate = rate;
                    Task::none()
                }

                CaptureMessage::FrameCaptured(frame) => {
                    self.frame_format = frame.format.clone();
                    self.frame_dimensions = frame.size;
                    self.frame_data = Some(Bytes::copy_from_slice(&frame.data));

                    if self.frame_sender.is_none() {
                        let Some(Ok(webrtc)) = &ctx.webrtc else {
                            tracing::error!("WebRTC is not initialized yet");
                            return Task::none();
                        };

                        let (tx, mut rx) = mpsc::channel::<Arc<Frame>>(2);
                        self.frame_sender = Some(tx.clone());

                        let webrtc = webrtc.clone();
                        let target_fps = self.capture_frame_rate.to_hz();
                        let bitrate = ctx.config.bitrate;

                        tokio::spawn(async move {
                            let mut encoder = match H264Encoder::new(bitrate, target_fps) {
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
            },

            Message::Error(err) => {
                if !err.is_empty() {
                    tracing::error!("Error: {}", err);
                }

                Task::none()
            }

            _ => Task::none(),
        }
    }

    fn view(&self, _ctx: &AppContext) -> Element<'_, Message> {
        let control_row: Element<Message> = container(
            row([
                if self.capturing {
                    button(text(self.capture_frame_rate.to_string()))
                        .style(iced::widget::button::secondary)
                        .into()
                } else {
                    pick_list(CaptureFramerate::ALL, Some(self.capture_frame_rate), |rate| {
                        Message::Capture(CaptureMessage::FrameRateSelected(rate))
                    })
                    .into()
                },
                button("Start Capture")
                    .on_press_maybe(if self.capturing {
                        None
                    } else {
                        Some(Message::Capture(CaptureMessage::StartCapture))
                    })
                    .into(),
                button("Stop Capture")
                    .on_press_maybe(if self.capturing {
                        Some(Message::Capture(CaptureMessage::StopCapture))
                    } else {
                        None
                    })
                    .into(),
            ])
            .spacing(10),
        )
        .padding(10)
        .center_x(Length::Fill)
        .into();

        let screen_share_preview = match &self.frame_data {
            Some(frame_data) => container(frame_viewer::frame_viewer(
                frame_data.clone(),
                self.frame_dimensions.x as u32,
                self.frame_dimensions.y as u32,
            ))
            .center(Length::Fill)
            .into(),

            None => container(text("No preview available.")).center(Length::Fill).into(),
        };

        iced::widget::column([control_row, screen_share_preview]).into()
    }
}

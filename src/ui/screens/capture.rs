use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use bytes::Bytes;
use iced::{
    Element, Length, Subscription, Task,
    widget::{button, container, pick_list, row, text},
};
use tokio::sync::Mutex;

use super::Screen;
use crate::{
    capture_providers::{
        CaptureProvider, PlatformCaptureProvider, PlatformCaptureStream, TARGET_PIXEL_FORMAT,
        shared::{CaptureFramerate, Frame, PixelFormat, Vector2},
        user_pick_platform_capture_item,
    },
    media::h264::H264Encoder,
    networking::webrtc::WebRTC,
    ui::{app::Message, frame_viewer},
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

pub struct CaptureScreen {
    capture: Arc<Mutex<PlatformCaptureProvider>>,

    pub active_window_handle: Option<u64>,
    pub capturing: bool,
    pub capture_frame_rate: CaptureFramerate,

    pub frame_data: Option<Bytes>,
    pub frame_dimensions: Vector2<i32>,
    pub frame_format: PixelFormat,

    pub webrtc: Option<WebRTC>,
    pub encoder: Option<H264Encoder>,
}

impl CaptureScreen {
    pub fn new(capture: Arc<Mutex<PlatformCaptureProvider>>) -> Self {
        Self {
            capture,
            active_window_handle: None,
            capturing: false,
            capture_frame_rate: CaptureFramerate::FPS60,
            frame_data: None,
            frame_dimensions: Vector2::new(0, 0),
            frame_format: TARGET_PIXEL_FORMAT,
            webrtc: None,
            encoder: None,
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

impl Screen for CaptureScreen {
    fn subscription(&self) -> Subscription<Message> {
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
                .map(|f| Message::FrameCaptured(Arc::new(f))),
            );
        }

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WebRTCInitialized(result) => {
                match result {
                    Ok(webrtc) => {
                        self.webrtc = Some(webrtc);
                        tracing::info!("WebRTC state initialized.");
                    }
                    Err(err) => {
                        tracing::error!("Failed to initialize WebRTC: {}", err);
                    }
                }
                Task::none()
            }

            Message::WindowOpened(id) => {
                iced::window::raw_id::<Message>(id).map(Message::WindowIdFetched)
            }
            Message::WindowIdFetched(id) => {
                self.active_window_handle = Some(id);
                Task::none()
            }

            Message::StartCapture => {
                let window_handle = match self.active_window_handle {
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

                    match H264Encoder::new() {
                        Ok(encoder) => {
                            self.encoder = Some(encoder);
                        }
                        Err(e) => {
                            return Task::done(Message::Error(format!(
                                "Failed to create H.264 encoder: {}",
                                e
                            )));
                        }
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
                self.capturing = true;
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
                self.capturing = false;
                self.encoder = None;
                Task::none()
            }

            Message::FrameRateSelected(rate) => {
                self.capture_frame_rate = rate;
                Task::none()
            }

            Message::FrameCaptured(frame) => {
                self.frame_format = frame.format.clone();
                self.frame_dimensions = frame.size;
                self.frame_data = Some(Bytes::copy_from_slice(&frame.data));

                let mut tasks = vec![];

                let (Some(encoder), Some(webrtc)) = (&mut self.encoder, &self.webrtc) else {
                    return Task::done(Message::Error(
                        "Encoder or WebRTC not initialized".to_owned(),
                    ));
                };

                match encoder.encode(&frame.data, frame.size.x, frame.size.y) {
                    Ok(nal_units) => {
                        let frame_timestamp = frame.timestamp.clone();
                        let frame_duration = self.capture_frame_rate.to_frametime();
                        for nal in nal_units {
                            let webrtc_clone = webrtc.clone();
                            tasks.push(Task::future(async move {
                                if let Err(e) = webrtc_clone
                                    .write_frame(Bytes::from(nal), frame_timestamp, frame_duration)
                                    .await
                                {
                                    tracing::error!("Failed to write frame to WebRTC track: {}", e);
                                }

                                Message::NoOp
                            }));
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to encode frame: {}", e);
                    }
                }

                Task::batch(tasks)
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

    fn view(&self) -> Element<'_, Message> {
        let control_row: Element<Message> = container(
            row([
                if self.capturing {
                    button(text(self.capture_frame_rate.to_string()))
                        .style(iced::widget::button::secondary)
                        .into()
                } else {
                    pick_list(
                        CaptureFramerate::ALL,
                        Some(self.capture_frame_rate),
                        Message::FrameRateSelected,
                    )
                    .into()
                },
                button("Start Capture")
                    .on_press_maybe(if self.capturing { None } else { Some(Message::StartCapture) })
                    .into(),
                button("Stop Capture")
                    .on_press_maybe(if self.capturing { Some(Message::StopCapture) } else { None })
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

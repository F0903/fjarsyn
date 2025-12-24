use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use iced::{
    Element, Length, Subscription, Task,
    widget::{button, container, stack, text},
};
use tokio::sync::{Mutex, mpsc};

use super::Screen;
use crate::{
    capture_providers::{
        CaptureProvider, PlatformCaptureProvider, PlatformCaptureStream,
        shared::{CaptureFramerate, Frame},
        user_pick_platform_capture_item,
    },
    media::h264_cpu::{H264Decoder, H264Encoder},
    networking::webrtc::WebRTCEvent,
    ui::{
        frame_viewer::FrameViewer,
        message::{Message, Route},
        state::AppContext,
    },
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

#[derive(Clone, Debug)]
pub struct CallScreen {
    capture: Arc<Mutex<PlatformCaptureProvider>>,

    pub capturing: bool,

    // Local Capture State
    pub local_frame: Option<Arc<Frame>>,
    pub frame_sender: Option<mpsc::Sender<Arc<Frame>>>,
    pub show_local_preview: bool,

    // Remote Capture State
    pub remote_frame: Option<Arc<Frame>>,
    pub decoder: Option<Arc<Mutex<H264Decoder>>>,
}

impl CallScreen {
    pub fn new(capture: Arc<Mutex<PlatformCaptureProvider>>) -> Self {
        Self {
            capture,
            capturing: false,

            local_frame: None,
            frame_sender: None,
            show_local_preview: false,

            remote_frame: None,
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
pub enum CallMessage {
    StartCapture,
    CaptureStarted,
    StopCapture,
    CaptureStopped,
    TryStartCapture(crate::capture_providers::PlatformCaptureItem),
    TryStopCapture,
    PlatformUserPickedCaptureItem(Result<crate::capture_providers::PlatformCaptureItem, String>),
    FrameCaptured(Arc<Frame>),
    DecodedFrameReady(Arc<Frame>),
    ToggleLocalPreview,
    EndCall,
}

impl Screen for CallScreen {
    fn subscription(&self, ctx: &AppContext) -> Subscription<Message> {
        let mut subscriptions = vec![];

        if self.capturing {
            subscriptions.push(
                Subscription::<Frame>::run_with(
                    FrameReceiverSubData {
                        capture: self.capture.clone(),
                        framerate: ctx.config.framerate,
                        stream_name: "frame-receiver",
                    },
                    Self::create_frame_receiver_subscription,
                )
                .map(|f| Message::Call(CallMessage::FrameCaptured(Arc::new(f)))),
            );
        }

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, ctx: &mut AppContext, message: Message) -> Task<Message> {
        match message {
            Message::PacketReceived(packet) => {
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
                        match lock.decode(&packet) {
                            Ok(Some(frame)) => Message::Call(CallMessage::DecodedFrameReady(frame)),
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

            Message::Call(msg) => match msg {
                CallMessage::DecodedFrameReady(frame) => {
                    self.remote_frame = Some(frame);
                    Task::none()
                }

                CallMessage::ToggleLocalPreview => {
                    self.show_local_preview = !self.show_local_preview;
                    Task::none()
                }

                CallMessage::EndCall => {
                    let stop_capture_task = if self.capturing {
                        Task::done(Message::Call(CallMessage::StopCapture))
                    } else {
                        Task::none()
                    };

                    let disconnect_task = if let Some(webrtc) = &ctx.webrtc {
                        let webrtc_clone = webrtc.clone();
                        Task::future(async move {
                            if let Err(e) = webrtc_clone.disconnect().await {
                                tracing::error!("Failed to disconnect WebRTC: {}", e);
                            }
                            Message::NoOp
                        })
                    } else {
                        Task::none()
                    };

                    Task::batch(vec![
                        stop_capture_task,
                        disconnect_task,
                        Task::done(Message::Navigate(Route::Home)),
                    ])
                }
                CallMessage::StartCapture => {
                    let window_handle = match ctx.main_window_handle {
                        Some(handle) => handle,
                        None => {
                            tracing::error!("No active window handle");
                            return Task::none();
                        }
                    };

                    match user_pick_platform_capture_item(window_handle) {
                        Ok(future) => Task::future(async move {
                            match future.await {
                                Ok(item) => Message::Call(
                                    CallMessage::PlatformUserPickedCaptureItem(Ok(item)),
                                ),
                                Err(e) => Message::Call(
                                    CallMessage::PlatformUserPickedCaptureItem(Err(e.to_string())),
                                ),
                            }
                        }),
                        Err(err) => {
                            tracing::error!("Failed to pick capture item: {}", err);
                            Task::none()
                        }
                    }
                }

                CallMessage::PlatformUserPickedCaptureItem(capture_item_result) => {
                    let capture_item = match capture_item_result {
                        Ok(item) => item,
                        Err(err) => {
                            tracing::error!("Failed to pick capture item: {}", err);
                            return Task::none();
                        }
                    };
                    Task::done(Message::Call(CallMessage::TryStartCapture(capture_item)))
                }

                CallMessage::TryStartCapture(capture_item) => match self.capture.try_lock() {
                    Ok(mut capture) => {
                        if let Err(err) = capture.set_capture_item(capture_item.clone()) {
                            tracing::error!("Failed to set capture item: {}", err);
                            return Task::none();
                        }

                        if let Err(err) = capture.start_capture() {
                            tracing::error!("Failed to start capture: {}", err);
                            return Task::none();
                        }

                        Task::done(Message::Call(CallMessage::CaptureStarted))
                    }
                    Err(_) => {
                        let capture_arc = self.capture.clone();
                        Task::future(async move {
                            let _lock = capture_arc.lock().await;
                        })
                        .map(move |_| {
                            Message::Call(CallMessage::TryStartCapture(capture_item.clone()))
                        })
                    }
                },

                CallMessage::CaptureStarted => {
                    self.capturing = true;
                    Task::none()
                }

                CallMessage::StopCapture => Task::done(Message::Call(CallMessage::TryStopCapture)),

                CallMessage::TryStopCapture => match self.capture.try_lock() {
                    Ok(mut capture) => {
                        if let Err(err) = capture.stop_capture() {
                            tracing::error!("Failed to stop capture: {}", err);
                        }
                        Task::done(Message::Call(CallMessage::CaptureStopped))
                    }
                    Err(_) => {
                        let capture_arc = self.capture.clone();
                        Task::future(async move {
                            let _lock = capture_arc.lock().await;
                            Message::Call(CallMessage::TryStopCapture)
                        })
                    }
                },

                CallMessage::CaptureStopped => {
                    self.capturing = false;
                    self.frame_sender = None;
                    self.local_frame = None;
                    Task::none()
                }

                CallMessage::FrameCaptured(frame) => {
                    self.local_frame = Some(frame.clone());

                    if self.frame_sender.is_none() {
                        let Some(webrtc) = &ctx.webrtc else {
                            tracing::error!("WebRTC is not initialized yet");
                            return Task::none();
                        };

                        let (tx, mut rx) = mpsc::channel::<Arc<Frame>>(10);
                        self.frame_sender = Some(tx.clone());

                        let webrtc = webrtc.clone();
                        let target_fps_hz = ctx.config.framerate.to_hz();
                        let bitrate = ctx.config.bitrate;

                        tracing::debug!(
                            "Starting encoder thread. target_fps_hz: {}, bitrate: {}",
                            target_fps_hz,
                            bitrate
                        );
                        tokio::spawn(async move {
                            let mut encoder = match H264Encoder::new(bitrate, target_fps_hz) {
                                Ok(enc) => enc,
                                Err(e) => {
                                    tracing::error!("Failed to create encoder: {}", e);
                                    return;
                                }
                            };

                            while let Some(frame) = rx.recv().await {
                                match encoder.encode(&frame.data, frame.size.x, frame.size.y) {
                                    Ok(nal_units) => {
                                        let frame_duration = match frame.duration {
                                            Some(duration) => duration,
                                            None => {
                                                tracing::error!("Frame duration is None!");
                                                continue;
                                            }
                                        };
                                        for nal in nal_units {
                                            if let Err(e) =
                                                webrtc.write_sample(nal, frame_duration).await
                                            {
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
            },

            // End the call if the peer disconnects
            Message::WebRTCEvent(WebRTCEvent::Disconnected) => {
                Task::done(Message::Call(CallMessage::EndCall))
            }

            _ => Task::none(),
        }
    }

    fn view(&self, _ctx: &AppContext) -> Element<'_, Message> {
        let mut controls_row: iced::widget::Row<'_, Message, iced::Theme, iced::Renderer> =
            iced::widget::Row::new()
                .push(button("Settings").on_press(Message::NavigateWithBack(Route::Settings)))
                .spacing(10);

        controls_row = if self.capturing {
            controls_row.extend([
                button("Change Screen").on_press(Message::Call(CallMessage::StartCapture)).into(),
                button(if self.show_local_preview { "Hide Preview" } else { "Show Preview" })
                    .on_press(Message::Call(CallMessage::ToggleLocalPreview))
                    .into(),
                button("Stop Sharing")
                    .style(iced::widget::button::danger)
                    .on_press(Message::Call(CallMessage::StopCapture))
                    .into(),
            ])
        } else {
            controls_row.extend([button("Share Screen")
                .on_press(Message::Call(CallMessage::StartCapture))
                .into()])
        };

        controls_row = controls_row.extend([button("End Call")
            .style(iced::widget::button::danger)
            .on_press(Message::Call(CallMessage::EndCall))
            .into()]);

        let controls_row: Element<'_, Message> =
            container(controls_row).padding(10).center_x(Length::Fill).into();

        let remote_view: Element<Message> = match self.remote_frame.clone() {
            Some(frame) => container(FrameViewer::new(frame)).center(Length::Fill).into(),
            None => container(text("Waiting for video...").size(30)).center(Length::Fill).into(),
        };

        let content = if let Some(local_frame) = self.local_frame.clone()
            && self.show_local_preview
        {
            let local_view = container(FrameViewer::new(local_frame))
                .width(Length::Fixed(320.0))
                .height(Length::Fixed(180.0))
                .style(container::bordered_box);

            stack![
                remote_view,
                container(local_view)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Bottom)
                    .padding(20),
                container(controls_row).width(Length::Fill).align_y(iced::alignment::Vertical::Top)
            ]
        } else {
            stack![
                remote_view,
                container(controls_row).width(Length::Fill).align_y(iced::alignment::Vertical::Top)
            ]
        };

        content.into()
    }
}

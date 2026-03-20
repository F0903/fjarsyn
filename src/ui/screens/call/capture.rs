use std::sync::Arc;

use iced::Task;

use super::{CallMessage, CallScreen};
use crate::{
    capture_providers::{CaptureProvider, PlatformCaptureItem, user_pick_platform_capture_item},
    ui::{
        app::{AppState, Fjarsyn},
        message::{Message, NavigationMessage, Route, ScreenMessage},
    },
};

impl CallScreen {
    pub(super) fn perform_end_call(&self, ctx: &AppState) -> Task<Message> {
        let stop_capture_task = self
            .capture
            .provider
            .clone()
            .map(|capture_arc| {
                Task::future(async move {
                    let mut capture = capture_arc.write().await;
                    if let Err(e) = capture.stop_capture() {
                        tracing::error!("Failed to stop capture: {}", e);
                    }
                    Message::NoOp
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

    pub(super) fn perform_initialize_capture(&self, ctx: &AppState) -> Task<Message> {
        Fjarsyn::init_capture_task(&ctx.config)
    }

    pub(super) fn perform_open_capture_picker(&self, window_handle: u64) -> Task<Message> {
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
            Err(err) => Task::done(Message::Screen(ScreenMessage::Call(
                CallMessage::PlatformUserPickedCaptureItem(Err(err.to_string())),
            ))),
        }
    }

    pub(super) fn perform_capture_start(
        &mut self,
        ctx: &mut AppState,
        capture_item: PlatformCaptureItem,
    ) -> Task<Message> {
        let capture_started =
            || Task::done(Message::Screen(ScreenMessage::Call(CallMessage::CaptureStarted)));

        match self.capture.provider.as_ref() {
            None => {
                ctx.notify_error("Screen capture is not available.");
                Task::none()
            }
            Some(capture) => match capture.try_write() {
                Ok(mut capture) => {
                    if let Err(err) = capture.set_capture_item(capture_item.clone()) {
                        ctx.notify_error(format!("Failed to select capture source: {}", err));
                        return Task::none();
                    }

                    if let Err(err) = capture.start_capture() {
                        ctx.notify_error(format!("Failed to start capture: {}", err));
                        return Task::none();
                    }

                    ctx.services
                        .call_service
                        .clone()
                        .map(|service| {
                            Task::future(async move {
                                if let Err(err) = service.webrtc().notify_stream_started().await {
                                    tracing::warn!(
                                        "Failed to notify remote peer that streaming started: {}",
                                        err
                                    );
                                }

                                Message::Screen(ScreenMessage::Call(CallMessage::CaptureStarted))
                            })
                        })
                        .unwrap_or_else(capture_started)
                }
                Err(_) => {
                    let capture_arc = self.capture.provider.clone();
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
        }
    }

    pub(super) fn perform_capture_stop(&mut self, ctx: &mut AppState) -> Task<Message> {
        match self.capture.provider.as_ref() {
            None => Task::done(Message::Screen(ScreenMessage::Call(CallMessage::CaptureStopped))),
            Some(capture) => match capture.try_write() {
                Ok(mut capture) => {
                    if let Err(err) = capture.stop_capture() {
                        ctx.notify_error(format!("Failed to stop capture: {}", err));
                    }

                    let notify_ended = ctx
                        .services
                        .call_service
                        .clone()
                        .map(|service| {
                            Task::future(async move {
                                if let Err(err) = service.webrtc().notify_stream_ended().await {
                                    tracing::warn!(
                                        "Failed to notify remote peer that streaming ended: {}",
                                        err
                                    );
                                }
                                Message::NoOp
                            })
                        })
                        .unwrap_or_else(Task::none);

                    Task::batch([
                        notify_ended,
                        Task::done(Message::Screen(ScreenMessage::Call(
                            CallMessage::CaptureStopped,
                        ))),
                    ])
                }
                Err(_) => {
                    tracing::debug!("Failed to acquire capture lock. Trying again with waiter...");
                    let capture_arc = self.capture.provider.clone();
                    Task::future(async move {
                        if let Some(capture_arc) = capture_arc {
                            let _lock = capture_arc.write().await;
                        }
                        Message::Screen(ScreenMessage::Call(CallMessage::TryStopCapture))
                    })
                }
            },
        }
    }

    pub(crate) fn set_capture_provider(
        &mut self,
        provider: Arc<tokio::sync::RwLock<crate::capture_providers::PlatformCaptureProvider>>,
    ) -> bool {
        self.capture.provider = Some(provider);
        if self.capture.pending_start {
            self.capture.pending_start = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn mark_capture_init_failed(&mut self) {
        self.capture.pending_start = false;
    }
}

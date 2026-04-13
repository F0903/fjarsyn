use std::sync::Arc;

use bytes::Bytes;
use fjarsyn_core::{
    capture_providers::windows::WgcCaptureProviderBuilder,
    config::Config,
    database,
    media::{ffmpeg::FFmpegTranscodeTypeExt, gpu_interop, pixel_format::PixelFormat},
    services::call_service::{CallEvent, CallService, CallServiceConfig},
};
use iced::{Task, window as iced_window};
use tokio::sync::{RwLock, mpsc};

use super::{Fjarsyn, bootstrap::AppBootstrap};
use crate::ui::message::Message;

impl Fjarsyn {
    pub(crate) fn startup_service_tasks(app: &Fjarsyn) -> Task<Message> {
        let frame_packet_tx = app.runtime.frame_packet_tx.clone();
        let call_event_tx = app.runtime.call_event_tx.clone();
        let max_depacket_latency = app.ctx.config.network.max_depacket_latency;
        let peer_id = app.ctx.config.identity.peer_id.clone();

        Task::batch([
            Task::future(async {
                use crate::ui::message::DatabaseMessage;
                Message::Database(DatabaseMessage::DatabaseInitialized(
                    database::init().await.map_err(Arc::new),
                ))
            }),
            Self::init_call_service_task(
                frame_packet_tx,
                call_event_tx,
                max_depacket_latency,
                peer_id,
            ),
        ])
    }

    fn capture_cpu_readback_enabled(config: &Config) -> bool {
        gpu_interop::requires_cpu_readback(
            config.capture.enable_ui_preview,
            PixelFormat::DEFAULT_CAPTURE,
            config.video.transcoding_type.get_encoder_info().hw_accel,
        )
    }

    pub fn init(config: Config) -> (Self, Task<Message>) {
        let bootstrap = AppBootstrap::new(config);
        let app = bootstrap.app;
        let startup_services = Self::startup_service_tasks(&app);

        (app, Task::batch([startup_services, Self::open_window_task(), Self::load_fonts_task()]))
    }

    fn init_call_service_task(
        frame_packet_tx: mpsc::Sender<Bytes>,
        call_event_tx: mpsc::Sender<CallEvent>,
        max_depacket_latency: u16,
        peer_id: Option<String>,
    ) -> Task<Message> {
        Task::future(async move {
            let config =
                CallServiceConfig { frame_packet_tx, call_event_tx, max_depacket_latency, peer_id };
            let res = CallService::init(config).await;
            use crate::ui::message::CallServiceMessage;
            Message::CallService(CallServiceMessage::CallServiceInitialized(
                res.map(Arc::new).map_err(Arc::new),
            ))
        })
    }

    pub(crate) fn init_capture_task(config: &Config) -> Task<Message> {
        let fmt = PixelFormat::DEFAULT_CAPTURE;
        let cursor = config.capture.record_cursor;
        let border = config.capture.recording_border_indicator;
        let cpu_readback_enabled = Self::capture_cpu_readback_enabled(config);
        Task::future(async move {
            let res = WgcCaptureProviderBuilder::new(fmt, cursor, border, cpu_readback_enabled)
                .with_default_device()
                .and_then(|b| b.with_default_capture_item())
                .and_then(|b| b.build())
                .map(|p| Arc::new(RwLock::new(p)));
            use crate::ui::message::CaptureMessage;
            Message::Capture(CaptureMessage::CaptureInitialized(
                res.map_err(|e| Arc::new(crate::Error::from(e))),
            ))
        })
    }

    fn open_window_task() -> Task<Message> {
        use crate::ui::message::WindowEventMessage;
        iced_window::open(iced_window::Settings {
            decorations: false,
            #[cfg(target_os = "windows")]
            platform_specific: iced_window::settings::PlatformSpecific {
                undecorated_shadow: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .1
        .map(|id| Message::WindowEvent(WindowEventMessage::WindowOpened(id)))
    }

    fn load_fonts_task() -> Task<Message> {
        use crate::ui::fonts::{geist, outfit};
        Task::batch([
            iced::font::load(iced_fonts::LUCIDE_FONT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::THIN_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::EXTRA_LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::REGULAR_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::MEDIUM_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::SEMIBOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::EXTRA_BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(outfit::BLACK_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::THIN_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::EXTRA_LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::LIGHT_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::REGULAR_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::MEDIUM_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::SEMIBOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::EXTRA_BOLD_BYTES).map(|_| Message::NoOp),
            iced::font::load(geist::BLACK_BYTES).map(|_| Message::NoOp),
        ])
    }
}

use std::sync::Arc;

use bytes::Bytes;
use iced::{Task, window as iced_window};
use tokio::sync::{RwLock, mpsc};

use super::{Fjarsyn, bootstrap::AppBootstrap};
use crate::{
    config::Config,
    media::pixel_format::PixelFormat,
    networking::discovery::{Discovery, DiscoveryEvent},
    services::call_service::{CallEvent, CallService, CallServiceConfig},
    ui::message::Message,
};

impl Fjarsyn {
    fn capture_cpu_readback_enabled(config: &Config) -> bool {
        crate::media::gpu_interop::requires_cpu_readback(
            config.capture.enable_ui_preview,
            PixelFormat::DEFAULT_CAPTURE,
            config.video.transcoding_type.get_encoder_info().hw_accel,
        )
    }

    pub fn init() -> (Self, Task<Message>) {
        let bootstrap = AppBootstrap::load();

        (
            bootstrap.app,
            Task::batch([
                Task::future(async {
                    use crate::ui::message::DatabaseMessage;
                    Message::Database(DatabaseMessage::DatabaseInitialized(
                        crate::database::init().await.map_err(Arc::new),
                    ))
                }),
                Self::init_call_service_task(
                    bootstrap.runtime.frame_packet_tx,
                    bootstrap.runtime.call_event_tx,
                    bootstrap.runtime.discovery_event_tx,
                    bootstrap.runtime.max_depacket_latency,
                    bootstrap.runtime.peer_id,
                ),
                Self::open_window_task(),
                Self::load_fonts_task(),
            ]),
        )
    }

    fn init_call_service_task(
        frame_packet_tx: mpsc::Sender<Bytes>,
        call_event_tx: mpsc::Sender<CallEvent>,
        discovery_event_tx: mpsc::Sender<DiscoveryEvent>,
        max_depacket_latency: u16,
        peer_id: Option<String>,
    ) -> Task<Message> {
        Task::future(async move {
            let config =
                CallServiceConfig { frame_packet_tx, call_event_tx, max_depacket_latency, peer_id };
            let res = CallService::init(config).await;
            if let Ok(ref service) = res
                && let Ok(d) = Discovery::new()
            {
                let _ = d.advertise(service.local_id(), service.signaling_port());
                let _ = d.browse(discovery_event_tx);
            }
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
            let res = crate::capture_providers::windows::WgcCaptureProviderBuilder::new(
                fmt,
                cursor,
                border,
                cpu_readback_enabled,
            )
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

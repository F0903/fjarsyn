use std::{collections::VecDeque, sync::Arc};

use iced::{Task, window};
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::{
    capture_providers::PlatformCaptureProvider,
    config::Config,
    networking::{
        discovery::{Discovery, DiscoveryEvent},
        webrtc::{WebRTC, WebRTCEvent},
    },
    ui::{
        message::{Message, Route},
        notification_provider::NotificationProvider,
        screens::{ActiveScreen, Screen},
        state::{AppContext, State, WindowInfo},
        subscription,
    },
};

pub fn init(capture: Arc<RwLock<PlatformCaptureProvider>>) -> (State, Task<Message>) {
    const REMOTE_FRAMES_BUFFER: usize = 100;
    const WEBRTC_EVENT_BUFFER: usize = 100;
    const DISCOVERY_EVENT_BUFFER: usize = 100;

    let (frame_tx, frame_rx) = mpsc::channel(REMOTE_FRAMES_BUFFER);
    let (event_tx, event_rx) = mpsc::channel(WEBRTC_EVENT_BUFFER);
    let (discovery_tx, discovery_rx) = mpsc::channel(DISCOVERY_EVENT_BUFFER);

    let config = Config::load();

    let mut ctx = AppContext {
        config,
        capture,
        main_window: None,
        back_queue: VecDeque::new(),
        packet_tx: Some(frame_tx),
        packet_rx: subscription::EventReceiverRef(Arc::new(Mutex::new(frame_rx))),
        webrtc_event_tx: Some(event_tx),
        webrtc_event_rx: Some(Arc::new(Mutex::new(event_rx))),
        discovery_event_tx: Some(discovery_tx.clone()),
        discovery_event_rx: Some(Arc::new(Mutex::new(discovery_rx))),
        webrtc: None,
        target_id: None,
        incoming_call_id: None,
        incoming_call_timeout: None,
        discovered_peers: Vec::new(),
        recent_peers: Vec::new(),
        notifications: NotificationProvider::new(),
    };

    let active_screen = ActiveScreen::Home(crate::ui::screens::home::HomeScreen::new(&mut ctx));

    let init_task = {
        let init_frame_tx = ctx.packet_tx.clone().unwrap();
        let init_event_tx = ctx.webrtc_event_tx.clone().unwrap();
        let max_depacket_latency = ctx.config.max_depacket_latency;

        Task::future(async move {
            let webrtc_result =
                WebRTC::init(init_frame_tx, init_event_tx, max_depacket_latency).await;
            if let Ok(ref webrtc) = webrtc_result {
                let id = webrtc.get_local_id();
                let port = webrtc.direct_signaling_port;
                if let Ok(discovery) = Discovery::new() {
                    let _ = discovery.advertise(&id, port);
                    let _ = discovery.browse(discovery_tx);
                }
            }
            webrtc_result
        })
        .map_err(Arc::new)
        .map(Message::WebRTCInitialized)
    };

    let window_settings = window::Settings {
        visible: true,
        transparent: false,
        blur: false,
        decorations: false,
        resizable: true,
        #[cfg(target_os = "windows")]
        platform_specific: window::settings::PlatformSpecific {
            undecorated_shadow: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let (_id, open_window_task) = window::open(window_settings);
    let open_window_task = open_window_task.map(Message::WindowOpened);

    let font_loads = Task::batch([
        // Lucide
        iced::font::load(iced_fonts::LUCIDE_FONT_BYTES).map(|_| Message::NoOp),
        // Outfit
        iced::font::load(crate::ui::fonts::outfit::THIN_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::outfit::EXTRA_LIGHT_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::outfit::LIGHT_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::outfit::REGULAR_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::outfit::MEDIUM_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::outfit::SEMIBOLD_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::outfit::BOLD_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::outfit::EXTRA_BOLD_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::outfit::BLACK_BYTES).map(|_| Message::NoOp),
        // Geist
        iced::font::load(crate::ui::fonts::geist::THIN_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::geist::EXTRA_LIGHT_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::geist::LIGHT_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::geist::REGULAR_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::geist::MEDIUM_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::geist::SEMIBOLD_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::geist::BOLD_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::geist::EXTRA_BOLD_BYTES).map(|_| Message::NoOp),
        iced::font::load(crate::ui::fonts::geist::BLACK_BYTES).map(|_| Message::NoOp),
    ]);

    (State { ctx, active_screen }, Task::batch([init_task, open_window_task, font_loads]))
}

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::Navigate(route) => {
            state.active_screen = ActiveScreen::from_route(route, &mut state.ctx);
            state.ctx.back_queue.clear();
            Task::none()
        }
        Message::NavigateWithBack(route) => {
            let mut screen = ActiveScreen::from_route(route, &mut state.ctx);
            std::mem::swap(&mut state.active_screen, &mut screen);
            state.ctx.back_queue.push_front(screen);
            Task::none()
        }
        Message::Back => {
            if let Some(screen) = state.ctx.back_queue.pop_front() {
                state.active_screen = screen;
            }
            Task::none()
        }
        Message::Tick(now) => handle_tick(state, now),
        Message::DismissNotification(id) => {
            state.ctx.notifications.dismiss(id);
            state.active_screen.update(&mut state.ctx, Message::DismissNotification(id))
        }
        Message::NotifyError(msg) => {
            state.ctx.notifications.error(msg);
            Task::none()
        }
        Message::NotifyInfo(msg) => {
            state.ctx.notifications.info(msg);
            Task::none()
        }
        Message::NotifySuccess(msg) => {
            state.ctx.notifications.success(msg);
            Task::none()
        }
        Message::WindowOpened(id) => handle_window_opened(state, id),
        Message::WindowClosed(id) => handle_window_closed(state, id),
        Message::WindowMaximized(maximized) => {
            if let Some(main_window) = state.ctx.main_window.as_mut() {
                main_window.maximized = maximized;
            }
            Task::none()
        }
        Message::SyncMaximized => {
            if let Some(main_window) = state.ctx.main_window.as_ref() {
                return window::is_maximized(main_window.iced_id).map(Message::WindowMaximized);
            }
            Task::none()
        }
        Message::WindowRawIdFetched((id, raw_id)) => handle_window_raw_id(state, id, raw_id),
        Message::Minimize => {
            if let Some(main_window) = state.ctx.main_window.as_ref() {
                return window::minimize(main_window.iced_id, true);
            }
            Task::none()
        }
        Message::Maximize => {
            if let Some(main_window) = state.ctx.main_window.as_ref() {
                return window::toggle_maximize(main_window.iced_id);
            }
            Task::none()
        }
        Message::Close => {
            if let Some(main_window) = state.ctx.main_window.as_ref() {
                return window::close(main_window.iced_id);
            }
            Task::none()
        }
        Message::Drag => {
            if let Some(main_window) = state.ctx.main_window.as_ref() {
                return window::drag(main_window.iced_id);
            }
            Task::none()
        }
        Message::Resize(direction) => {
            if let Some(main_window) = state.ctx.main_window.as_ref() {
                return window::drag_resize(main_window.iced_id, direction);
            }
            Task::none()
        }
        Message::WebRTCInitialized(ref result) => {
            handle_webrtc_initialized(state, result.clone(), message)
        }
        Message::AcceptCall => handle_accept_call(state),
        Message::DeclineCall => handle_decline_call(state),
        Message::StartCall(target) => handle_start_call(state, target),
        Message::WebRTCEvent(ref event) => handle_webrtc_event(state, event.clone(), message),
        Message::DiscoveryEvent(ref event) => handle_discovery_event(state, event.clone()),
        _ => state.active_screen.update(&mut state.ctx, message),
    }
}

fn handle_tick(state: &mut State, now: std::time::Instant) -> Task<Message> {
    state.ctx.notifications.dismiss_expired(now);
    if let Some(timeout) = state.ctx.incoming_call_timeout {
        if now > timeout {
            tracing::info!("Incoming call timed out");
            state.ctx.notifications.info("Missed call.");
            return Task::done(Message::DeclineCall);
        }
    }

    state.active_screen.update(&mut state.ctx, Message::Tick(now))
}

fn handle_window_opened(state: &mut State, id: iced::window::Id) -> Task<Message> {
    if state.ctx.main_window.is_none() {
        state.ctx.main_window = Some(WindowInfo { iced_id: id, raw_id: None, maximized: false });
    }
    Task::batch([
        iced::window::raw_id::<Message>(id)
            .map(move |raw_id| Message::WindowRawIdFetched((id, raw_id))),
        state.active_screen.update(&mut state.ctx, Message::WindowOpened(id)),
    ])
}

fn handle_window_closed(state: &mut State, id: iced::window::Id) -> Task<Message> {
    let mut is_main = false;
    if let Some(main_window) = state.ctx.main_window.as_ref() {
        if main_window.iced_id == id {
            is_main = true;
        }
    }

    if is_main {
        state.ctx.main_window = None;
        return iced::exit();
    }
    state.active_screen.update(&mut state.ctx, Message::WindowClosed(id))
}

fn handle_window_raw_id(state: &mut State, id: iced::window::Id, raw_id: u64) -> Task<Message> {
    if let Some(main_window) = state.ctx.main_window.as_mut() {
        if main_window.iced_id == id {
            main_window.raw_id = Some(raw_id);
        }
    }
    state.active_screen.update(&mut state.ctx, Message::WindowRawIdFetched((id, raw_id)))
}

fn handle_webrtc_initialized(
    state: &mut State,
    result: Result<WebRTC, Arc<crate::networking::webrtc::WebRTCError>>,
    original_msg: Message,
) -> Task<Message> {
    match result {
        Ok(webrtc) => {
            tracing::info!("WebRTC P2P listener initialized.");
            state.ctx.webrtc = Some(webrtc);
        }
        Err(err) => {
            let err_msg = format!("Failed to initialize WebRTC: {}", err);
            tracing::error!(err_msg);
            state.ctx.notifications.error(err_msg);
        }
    }
    state.active_screen.update(&mut state.ctx, original_msg)
}

fn handle_accept_call(state: &mut State) -> Task<Message> {
    state.ctx.incoming_call_id = None;
    state.ctx.incoming_call_timeout = None;
    if let Some(webrtc) = &state.ctx.webrtc {
        let webrtc_clone = webrtc.clone();
        Task::future(async move {
            match webrtc_clone.accept_call().await {
                Ok(_) => Message::Navigate(Route::Call),
                Err(e) => {
                    tracing::error!("Failed to accept call: {}", e);
                    Message::NoOp
                }
            }
        })
    } else {
        Task::none()
    }
}

fn handle_decline_call(state: &mut State) -> Task<Message> {
    state.ctx.incoming_call_id = None;
    state.ctx.incoming_call_timeout = None;
    if let Some(webrtc) = &state.ctx.webrtc {
        let webrtc_clone = webrtc.clone();
        Task::future(async move {
            let _ = webrtc_clone.decline_call().await;
            Message::NoOp
        })
    } else {
        Task::none()
    }
}

fn handle_webrtc_event(
    state: &mut State,
    event: WebRTCEvent,
    original_msg: Message,
) -> Task<Message> {
    match event {
        WebRTCEvent::IncomingCall(sender) => {
            tracing::info!("Incoming call from {}", sender);
            state.ctx.target_id = Some(sender.clone());
            state.ctx.incoming_call_id = Some(sender.clone());
            state.ctx.incoming_call_timeout =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
        }
        WebRTCEvent::Connected => {
            tracing::info!("WebRTC Connected!");
            state.ctx.incoming_call_id = None;
            state.ctx.incoming_call_timeout = None;
            if let Some(target_id) = &state.ctx.target_id {
                if let Some(peer) =
                    state.ctx.discovered_peers.iter().find(|p| p.id == *target_id).cloned()
                {
                    state.ctx.recent_peers.retain(|p| p.id != peer.id);
                    state.ctx.recent_peers.insert(0, peer);
                }
            }
            if let ActiveScreen::Home(_) = state.active_screen {
                state.active_screen = ActiveScreen::from_route(Route::Call, &mut state.ctx);
            }
        }
        WebRTCEvent::Disconnected => {
            tracing::info!("WebRTC Disconnected");
            if state.ctx.target_id.is_some() {
                state.ctx.notifications.info("Call ended or declined.");
            }
            state.ctx.target_id = None;
            state.ctx.incoming_call_id = None;
            state.ctx.incoming_call_timeout = None;
        }
    }
    state.active_screen.update(&mut state.ctx, original_msg)
}

fn handle_discovery_event(state: &mut State, event: DiscoveryEvent) -> Task<Message> {
    match event {
        DiscoveryEvent::PeerFound(peer) => {
            let is_self =
                state.ctx.webrtc.as_ref().map(|w| w.get_local_id() == peer.id).unwrap_or(false);
            if !is_self {
                if let Some(existing) =
                    state.ctx.discovered_peers.iter_mut().find(|p| p.id == peer.id)
                {
                    existing.update(peer);
                } else {
                    state.ctx.discovered_peers.push(peer);
                }
            }
        }
        DiscoveryEvent::PeerRemoved(fullname) => {
            state.ctx.discovered_peers.retain(|p| !fullname.contains(&p.instance_name));
        }
    }
    Task::none()
}

fn handle_start_call(state: &mut State, target: crate::ui::message::CallTarget) -> Task<Message> {
    use crate::ui::message::CallTarget;

    let webrtc = match &state.ctx.webrtc {
        Some(webrtc) => webrtc.clone(),
        None => {
            tracing::warn!("WebRTC not initialized...");
            return Task::none();
        }
    };

    let discovered_peers = state.ctx.discovered_peers.clone();

    Task::future(async move {
        match target {
            CallTarget::Address(addr_str) => {
                let addr = match addr_str.parse::<std::net::SocketAddr>() {
                    Ok(addr) => addr,
                    Err(e) => {
                        let err_msg = format!("Failed to parse address '{}': {}", addr_str, e);
                        tracing::error!(err_msg);
                        return Message::NotifyError(err_msg);
                    }
                };

                tracing::info!("Attempting manual direct dial to {}", addr);
                if let Err(e) = webrtc.dial_direct(addr).await {
                    let err_msg = format!("Manual dial failed: {}", e);
                    tracing::error!(err_msg);
                    return Message::NotifyError(err_msg);
                }
            }
            CallTarget::PeerId(id) => {
                let discovered_peer = discovered_peers.iter().find(|p| p.id == id).cloned();

                let peer = match discovered_peer {
                    Some(peer) => peer,
                    None => {
                        let err_msg = format!("Peer ID '{}' was not found in discovered peers", id);
                        tracing::error!(err_msg);
                        return Message::NotifyError(err_msg);
                    }
                };

                let mut success = false;
                for addr in &peer.addresses {
                    let socket_addr = std::net::SocketAddr::new(*addr, peer.port);
                    tracing::info!("Attempting direct signaling to {}", socket_addr);
                    if let Ok(_) = webrtc.dial_direct(socket_addr).await {
                        tracing::info!("Successfully connected to signaling at {}", socket_addr);
                        success = true;
                        break;
                    }
                }

                if !success {
                    let err_msg = format!("Failed to connect to any address for peer {}", id);
                    tracing::error!(err_msg);
                    return Message::NotifyError(err_msg);
                }
            }
        }

        match webrtc.create_offer().await {
            Ok(_) => Message::Navigate(Route::Call),
            Err(e) => {
                let err_msg = format!("Failed to create offer: {}", e);
                tracing::error!(err_msg);
                return Message::NotifyError(err_msg);
            }
        }
    })
}

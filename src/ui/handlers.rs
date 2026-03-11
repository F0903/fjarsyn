use std::{collections::VecDeque, net::SocketAddr, sync::Arc};

use iced::{Task, window};
use tokio::sync::{Mutex, RwLock, mpsc};

use crate::{
    capture_providers::PlatformCaptureProvider,
    config::Config,
    networking::{
        discovery::{Discovery, DiscoveryEvent, PeerInfo},
        webrtc::{WebRTC, WebRTCEvent},
    },
    ui::{
        message::{CallTarget, Message, Route},
        notification_provider::NotificationProvider,
        screens::{ActiveScreen, Screen},
        state::{AppContext, State, WindowInfo},
        subscription,
    },
};

pub fn init() -> (State, Task<Message>) {
    let (ftx, frx) = mpsc::channel(100);
    let (etx, erx) = mpsc::channel(100);
    let (dtx, drx) = mpsc::channel(100);

    let config = Config::load();
    let mut ctx = AppContext {
        config: config.clone(),
        db: None,
        capture: None,
        main_window: None,
        back_queue: VecDeque::new(),
        packet_tx: Some(ftx.clone()),
        packet_rx: subscription::EventReceiverRef(Arc::new(Mutex::new(frx))),
        webrtc_event_tx: Some(etx.clone()),
        webrtc_event_rx: Some(Arc::new(Mutex::new(erx))),
        discovery_event_tx: Some(dtx.clone()),
        discovery_event_rx: Some(Arc::new(Mutex::new(drx))),
        webrtc: None,
        target_id: None,
        incoming_call_id: None,
        incoming_call_timeout: None,
        discovered_peers: Vec::new(),
        recent_peers: Vec::new(),
        contacts: Vec::new(),
        notifications: NotificationProvider::new(),
    };

    let active_screen = ActiveScreen::Home(crate::ui::screens::home::HomeScreen::new(&mut ctx));

    (
        State { ctx, active_screen },
        Task::batch([
            Task::future(async {
                Message::DatabaseInitialized(crate::database::init().await.map_err(Arc::new))
            }),
            init_capture_task(&config),
            init_webrtc_task(ftx, etx, dtx, config.max_depacket_latency, config.peer_id),
            open_window_task(),
            load_fonts_task(),
        ]),
    )
}

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match &message {
        Message::Navigate(route) => {
            state.active_screen = ActiveScreen::from_route(*route, &mut state.ctx);
            state.ctx.back_queue.clear();
            Task::none()
        }
        Message::NavigateWithBack(route) => {
            let mut screen = ActiveScreen::from_route(*route, &mut state.ctx);
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
        Message::Tick(now) => handle_tick(state, *now),
        Message::DismissNotification(id) => {
            state.ctx.notifications.dismiss(*id);
            state.active_screen.update(&mut state.ctx, message)
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

        Message::WindowOpened(id) => handle_window_opened(state, *id),
        Message::WindowClosed(id) => handle_window_closed(state, *id),
        Message::WindowMaximized(max) => {
            if let Some(w) = state.ctx.main_window.as_mut() {
                w.maximized = *max;
            }
            Task::none()
        }
        Message::SyncMaximized => state
            .ctx
            .main_window
            .as_ref()
            .map(|w| window::is_maximized(w.iced_id).map(Message::WindowMaximized))
            .unwrap_or(Task::none()),
        Message::WindowRawIdFetched((id, rid)) => handle_window_raw_id(state, *id, *rid),

        Message::Minimize => window_action(state, |id| window::minimize(id, true)),
        Message::Maximize => window_action(state, window::toggle_maximize),
        Message::Close => window_action(state, window::close),
        Message::Drag => window_action(state, window::drag),
        Message::Resize(dir) => window_action(state, |id| window::drag_resize(id, *dir)),

        Message::DatabaseInitialized(res) => handle_db_init(state, res.clone()),
        Message::CaptureInitialized(res) => handle_capture_init(state, res.clone()),
        Message::LoadContacts => handle_load_contacts(state),
        Message::ContactsLoaded(res) => handle_contacts_loaded(state, res.clone(), &message),
        Message::SaveContact { peer_id, name, address } => {
            handle_save_contact(state, peer_id.clone(), name.clone(), address.clone())
        }
        Message::ContactSaved(res) => handle_contact_saved(state, res.clone(), &message),
        Message::DeleteContact(id) => handle_delete_contact(state, *id),
        Message::ContactDeleted(res) => handle_contact_deleted(state, res.clone(), &message),
        Message::UpdateContactAddress { id, new_address } => {
            handle_update_address(state, *id, new_address.clone())
        }
        Message::UpdateContactAddressConfirmed(id, addr) => {
            handle_update_confirmed(state, *id, addr.clone())
        }

        Message::WebRTCInitialized(res) => handle_webrtc_init(state, res.clone(), &message),
        Message::AcceptCall => handle_accept_call(state),
        Message::DeclineCall => handle_decline_call(state),
        Message::StartCall(target) => handle_start_call(state, target.clone()),
        Message::WebRTCEvent(event) => handle_webrtc_event(state, event.clone(), &message),
        Message::DiscoveryEvent(event) => handle_discovery_event(state, event.clone()),
        Message::PeerFound(peer) => {
            handle_peer_found(state, peer.clone());
            Task::none()
        }
        Message::PeerRemoved(id) => {
            state.ctx.discovered_peers.retain(|p| p.id != *id);
            Task::none()
        }

        Message::Batch(messages) => {
            Task::batch(messages.clone().into_iter().map(|msg| update(state, msg)))
        }
        _ => state.active_screen.update(&mut state.ctx, message),
    }
}


fn window_action(state: &State, f: impl FnOnce(window::Id) -> Task<Message>) -> Task<Message> {
    state.ctx.main_window.as_ref().map(|w| f(w.iced_id)).unwrap_or(Task::none())
}

fn handle_db_init(
    state: &mut State,
    res: Result<sqlx::SqlitePool, Arc<crate::Error>>,
) -> Task<Message> {
    match res {
        Ok(pool) => {
            state.ctx.db = Some(pool);
            Task::done(Message::LoadContacts)
        }
        Err(e) => {
            state.ctx.notifications.error(format!("DB Failed: {}", e));
            Task::none()
        }
    }
}

fn handle_capture_init(
    state: &mut State,
    res: Result<Arc<RwLock<PlatformCaptureProvider>>, Arc<crate::Error>>,
) -> Task<Message> {
    match res {
        Ok(provider) => {
            state.ctx.capture = Some(provider);
            tracing::info!("Capture ready.");
        }
        Err(e) => {
            state.ctx.notifications.error(format!("Capture Failed: {}", e));
        }
    }
    Task::none()
}

fn handle_load_contacts(state: &State) -> Task<Message> {
    let db = match &state.ctx.db {
        Some(db) => db.clone(),
        None => return Task::none(),
    };
    Task::future(async move {
        Message::ContactsLoaded(crate::database::Contact::list(&db).await.map_err(Arc::new))
    })
}

fn handle_contacts_loaded(
    state: &mut State,
    res: Result<Vec<crate::database::Contact>, Arc<crate::Error>>,
    msg: &Message,
) -> Task<Message> {
    if let Ok(c) = res {
        state.ctx.contacts = c;
    }
    state.active_screen.update(&mut state.ctx, msg.clone())
}

fn handle_save_contact(
    state: &State,
    peer_id: String,
    name: String,
    address: Option<String>,
) -> Task<Message> {
    let db = match &state.ctx.db {
        Some(db) => db.clone(),
        None => return Task::none(),
    };
    Task::future(async move {
        Message::ContactSaved(
            crate::database::Contact::create(&db, peer_id, name, address).await.map_err(Arc::new),
        )
    })
}

fn handle_contact_saved(
    state: &mut State,
    res: Result<i64, Arc<crate::Error>>,
    msg: &Message,
) -> Task<Message> {
    if res.is_ok() {
        state.ctx.notifications.success("Contact saved.");
        return Task::batch([
            Task::done(Message::LoadContacts),
            state.active_screen.update(&mut state.ctx, msg.clone()),
        ]);
    }
    state.active_screen.update(&mut state.ctx, msg.clone())
}

fn handle_delete_contact(state: &State, id: i64) -> Task<Message> {
    let db = match &state.ctx.db {
        Some(db) => db.clone(),
        None => return Task::none(),
    };
    Task::future(async move {
        Message::ContactDeleted(crate::database::Contact::delete(&db, id).await.map_err(Arc::new))
    })
}

fn handle_contact_deleted(
    state: &mut State,
    res: Result<(), Arc<crate::Error>>,
    msg: &Message,
) -> Task<Message> {
    if res.is_ok() {
        state.ctx.notifications.success("Contact deleted.");
        return Task::batch([
            Task::done(Message::LoadContacts),
            state.active_screen.update(&mut state.ctx, msg.clone()),
        ]);
    }
    state.active_screen.update(&mut state.ctx, msg.clone())
}

fn handle_update_address(state: &mut State, id: i64, addr: String) -> Task<Message> {
    if let Some(c) = state.ctx.contacts.iter().find(|c| c.id == id) {
        state.ctx.notifications.info(format!("Updating address for {}...", c.name));
        return Task::done(Message::UpdateContactAddressConfirmed(id, addr));
    }
    Task::none()
}

fn handle_update_confirmed(state: &mut State, id: i64, addr: String) -> Task<Message> {
    let db = match &state.ctx.db {
        Some(db) => db.clone(),
        None => return Task::none(),
    };
    let c = match state.ctx.contacts.iter().find(|c| c.id == id) {
        Some(c) => c.clone(),
        None => return Task::none(),
    };
    Task::future(async move {
        let res = crate::database::Contact::update(&db, id, c.peer_id, c.name, Some(addr)).await;
        match res {
            Ok(_) => Message::LoadContacts,
            Err(e) => Message::NotifyError(format!("Update Failed: {}", e)),
        }
    })
}

fn handle_tick(state: &mut State, now: std::time::Instant) -> Task<Message> {
    state.ctx.notifications.dismiss_expired(now);
    if state.ctx.incoming_call_timeout.map(|t| now > t).unwrap_or(false) {
        state.ctx.notifications.info("Missed call.");
        return Task::done(Message::DeclineCall);
    }
    state.active_screen.update(&mut state.ctx, Message::Tick(now))
}

fn handle_window_opened(state: &mut State, id: iced::window::Id) -> Task<Message> {
    if state.ctx.main_window.is_none() {
        state.ctx.main_window = Some(WindowInfo { iced_id: id, raw_id: None, maximized: false });
    }
    Task::batch([
        iced::window::raw_id::<Message>(id).map(move |rid| Message::WindowRawIdFetched((id, rid))),
        state.active_screen.update(&mut state.ctx, Message::WindowOpened(id)),
    ])
}

fn handle_window_closed(state: &mut State, id: iced::window::Id) -> Task<Message> {
    if state.ctx.main_window.as_ref().map(|w| w.iced_id == id).unwrap_or(false) {
        state.ctx.main_window = None;
        return iced::exit();
    }
    state.active_screen.update(&mut state.ctx, Message::WindowClosed(id))
}

fn handle_window_raw_id(state: &mut State, id: iced::window::Id, raw_id: u64) -> Task<Message> {
    if let Some(w) = state.ctx.main_window.as_mut().filter(|w| w.iced_id == id) {
        w.raw_id = Some(raw_id);
    }
    state.active_screen.update(&mut state.ctx, Message::WindowRawIdFetched((id, raw_id)))
}

fn handle_webrtc_init(
    state: &mut State,
    res: Result<WebRTC, Arc<crate::networking::webrtc::WebRTCError>>,
    msg: &Message,
) -> Task<Message> {
    if let Ok(w) = &res {
        if state.ctx.config.peer_id.is_none() {
            state.ctx.config.peer_id = Some(w.get_local_id());
            let _ = state.ctx.config.save();
        }
        state.ctx.webrtc = Some(w.clone());
    }
    state.active_screen.update(&mut state.ctx, msg.clone())
}

fn handle_accept_call(state: &mut State) -> Task<Message> {
    state.ctx.incoming_call_id = None;
    state.ctx.incoming_call_timeout = None;
    let webrtc = match &state.ctx.webrtc {
        Some(w) => w.clone(),
        None => return Task::none(),
    };
    Task::future(async move {
        match webrtc.accept_call().await {
            Ok(_) => Message::Navigate(Route::Call),
            Err(_) => Message::NoOp,
        }
    })
}

fn handle_decline_call(state: &mut State) -> Task<Message> {
    state.ctx.incoming_call_id = None;
    state.ctx.incoming_call_timeout = None;
    let webrtc = match &state.ctx.webrtc {
        Some(w) => w.clone(),
        None => return Task::none(),
    };
    Task::future(async move {
        let _ = webrtc.decline_call().await;
        Message::NoOp
    })
}

fn handle_webrtc_event(state: &mut State, event: WebRTCEvent, msg: &Message) -> Task<Message> {
    match &event {
        WebRTCEvent::IncomingCall(sender) => {
            state.ctx.target_id = Some(sender.clone());
            state.ctx.incoming_call_id = Some(sender.clone());
            state.ctx.incoming_call_timeout =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
        }
        WebRTCEvent::Connected => {
            state.ctx.incoming_call_id = None;
            state.ctx.incoming_call_timeout = None;
            if let Some(tid) = &state.ctx.target_id {
                if let Some(p) = state.ctx.discovered_peers.iter().find(|p| p.id == *tid).cloned() {
                    state.ctx.recent_peers.retain(|rp| rp.id != p.id);
                    state.ctx.recent_peers.insert(0, p);
                }
            }
            if matches!(state.active_screen, ActiveScreen::Home(_)) {
                state.active_screen = ActiveScreen::from_route(Route::Call, &mut state.ctx);
            }
        }
        WebRTCEvent::Disconnected => {
            if state.ctx.target_id.is_some() {
                state.ctx.notifications.info("Call ended.");
            }
            state.ctx.target_id = None;
            state.ctx.incoming_call_id = None;
            state.ctx.incoming_call_timeout = None;
        }
    }
    state.active_screen.update(&mut state.ctx, msg.clone())
}

fn handle_discovery_event(state: &mut State, event: DiscoveryEvent) -> Task<Message> {
    match event {
        DiscoveryEvent::PeerFound(peer) => handle_peer_found(state, peer),
        DiscoveryEvent::PeerRemoved(fullname) => {
            state.ctx.discovered_peers.retain(|p| !fullname.contains(&p.instance_name))
        }
    }
    Task::none()
}

fn handle_peer_found(state: &mut State, peer: PeerInfo) {
    if state.ctx.webrtc.as_ref().map(|w| w.get_local_id() == peer.id).unwrap_or(false) {
        return;
    }
    if let Some(existing) = state.ctx.discovered_peers.iter_mut().find(|p| p.id == peer.id) {
        existing.update(peer);
    } else {
        state.ctx.discovered_peers.push(peer);
    }
}

fn handle_start_call(state: &mut State, target: CallTarget) -> Task<Message> {
    let webrtc = match &state.ctx.webrtc {
        Some(w) => w.clone(),
        None => return Task::none(),
    };
    let discovered = state.ctx.discovered_peers.clone();
    let contacts = state.ctx.contacts.clone();

    Task::future(async move {
        let (tid, taddr, tname) = match resolve_target(&target, &contacts) {
            Ok(res) => res,
            Err(e) => return e,
        };

        match dial_logic(&webrtc, &tid, &taddr, &discovered, &target).await {
            Ok((msgs, saddr)) => {
                let mut batch = msgs;
                if let (Some(id), Some(name), Some(addr)) = (tid, tname, saddr) {
                    batch.push(Message::PeerFound(PeerInfo {
                        id: id.clone(),
                        instance_name: name.clone(),
                        host_name: "direct".into(),
                        addresses: vec![addr.ip()],
                        port: addr.port(),
                    }));
                }
                match webrtc.create_offer().await {
                    Ok(_) => {
                        batch.push(Message::Navigate(Route::Call));
                        Message::Batch(batch)
                    }
                    Err(e) => Message::NotifyError(format!("Offer failed: {}", e)),
                }
            }
            Err(e_msg) => {
                let mut batch = vec![Message::NotifyError(e_msg)];
                if let Some(id) = tid {
                    batch.push(Message::PeerRemoved(id));
                }
                Message::Batch(batch)
            }
        }
    })
}

fn resolve_target(
    target: &CallTarget,
    contacts: &[crate::database::Contact],
) -> Result<(Option<String>, Option<String>, Option<String>), Message> {
    match target {
        CallTarget::ContactId(id) => contacts
            .iter()
            .find(|c| c.id == *id)
            .map(|c| (Some(c.peer_id.clone()), c.address.clone(), Some(c.name.clone())))
            .ok_or_else(|| Message::NotifyError("Contact not found".into())),
        CallTarget::PeerId(id) => Ok((Some(id.clone()), None, None)),
        CallTarget::Address(addr) => Ok((None, Some(addr.clone()), None)),
    }
}

async fn dial_logic(
    webrtc: &WebRTC,
    tid: &Option<String>,
    taddr: &Option<String>,
    discovered: &[PeerInfo],
    target: &CallTarget,
) -> Result<(Vec<Message>, Option<SocketAddr>), String> {
    let mut msgs = Vec::new();
    if let Some(id) = tid {
        if let Some(p) = discovered.iter().find(|p| p.id == *id) {
            for addr in &p.addresses {
                let saddr = SocketAddr::new(*addr, p.port);
                if webrtc.dial_direct(saddr).await.is_ok() {
                    if let CallTarget::ContactId(cid) = target {
                        let s = saddr.to_string();
                        if taddr.as_ref() != Some(&s) {
                            msgs.push(Message::UpdateContactAddress { id: *cid, new_address: s });
                        }
                    }
                    return Ok((msgs, None));
                }
            }
        }
    }
    if let Some(addr_str) = taddr {
        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
            if webrtc.dial_direct(addr).await.is_ok() {
                return Ok((msgs, Some(addr)));
            }
        } else {
            return Err("Invalid address format".into());
        }
    }
    Err("Connection failed".into())
}

fn init_webrtc_task(
    ftx: mpsc::Sender<bytes::Bytes>,
    etx: mpsc::Sender<WebRTCEvent>,
    dtx: mpsc::Sender<DiscoveryEvent>,
    lat: u16,
    pid: Option<String>,
) -> Task<Message> {
    Task::future(async move {
        let res = WebRTC::init(ftx, etx, lat, pid).await;
        if let Ok(ref w) = res {
            if let Ok(d) = Discovery::new() {
                let _ = d.advertise(&w.get_local_id(), w.direct_signaling_port);
                let _ = d.browse(dtx);
            }
        }
        Message::WebRTCInitialized(res.map_err(Arc::new))
    })
}

fn init_capture_task(config: &Config) -> Task<Message> {
    let fmt = config.pixel_format;
    let cursor = config.record_cursor;
    let border = config.recording_border_indicator;
    Task::future(async move {
        let res =
            crate::capture_providers::windows::WgcCaptureProviderBuilder::new(fmt, cursor, border)
                .with_default_device()
                .and_then(|b| b.with_default_capture_item())
                .and_then(|b| b.build())
                .map(|p| Arc::new(RwLock::new(p)));
        Message::CaptureInitialized(res.map_err(|e| Arc::new(crate::Error::from(e))))
    })
}

fn open_window_task() -> Task<Message> {
    window::open(window::Settings {
        decorations: false,
        #[cfg(target_os = "windows")]
        platform_specific: window::settings::PlatformSpecific {
            undecorated_shadow: true,
            ..Default::default()
        },
        ..Default::default()
    })
    .1
    .map(Message::WindowOpened)
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

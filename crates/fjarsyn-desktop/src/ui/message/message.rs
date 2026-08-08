use std::sync::Arc;

use fjarsyn_engine::config;

use super::{ContactOperation, Route, Screen, peer, window};
use crate::ui::runtime;

#[derive(Debug, Clone)]
pub(in crate::ui) enum Config {
    SaveRequested(config::Config),
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Lifecycle {
    RetryStartup,
    RestartRequested,
    ExitRequested,
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Navigation {
    Navigate(Route),
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Notification {
    NotifyError(String),
    NotifyInfo(String),
    Dismiss(u64),
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Runtime {
    Initialized(Result<runtime::Slot, Arc<String>>),
    Event(runtime::Event),
    ShutdownFinished(Result<(), Arc<String>>),
    RestartFinished {
        shutdown_warning: Option<Arc<String>>,
        launch_result: Result<(), Arc<String>>,
    },
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Message {
    Navigation(Navigation),
    Lifecycle(Lifecycle),
    Config(Config),
    Screen(Screen),
    PeerAction(peer::Action),
    Runtime(Runtime),
    Notification(Notification),
    ContactOperation(ContactOperation),
    WindowEvent(window::Event),
    WindowControl(window::Control),

    CopyId(String),
    CopyInvite(String),
    CopyFingerprint(String),
    Tick(std::time::Instant),
    NoOp,
}

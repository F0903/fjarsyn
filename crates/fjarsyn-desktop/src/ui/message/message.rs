use std::sync::Arc;

use super::{ContactOperation, Route, Screen, peer, window};
use crate::{settings, ui::runtime};

#[derive(Debug, Clone)]
pub(in crate::ui) enum Settings {
    SaveRequested(settings::Settings),
    SaveAndRetryRequested(settings::Settings),
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
    Initialized {
        runtime_id: runtime::RuntimeId,
        result: Result<runtime::RuntimeSlot, Arc<String>>,
    },
    EngineStateChanged {
        runtime_id: runtime::RuntimeId,
    },
    EngineNotice {
        runtime_id: runtime::RuntimeId,
        notice: runtime::EngineNotice,
    },
    EngineAdapterFailed {
        runtime_id: runtime::RuntimeId,
        failure: runtime::EngineAdapterFailure,
    },
    ShutdownFinished(Result<(), Arc<String>>),
    RestartShutdownFinished(Result<(), Arc<String>>),
}

#[derive(Debug, Clone)]
pub(in crate::ui) enum Message {
    Navigation(Navigation),
    Lifecycle(Lifecycle),
    Settings(Settings),
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

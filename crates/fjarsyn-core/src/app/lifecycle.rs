use super::{AppState, ServicePhase, ServicesState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppLifecycle {
    #[default]
    Bootstrapping,
    Ready,
    Degraded,
    Failed,
    ShuttingDown,
}

impl AppState {
    #[inline]
    pub fn accepts_user_requests(&self) -> bool {
        !matches!(self.lifecycle, AppLifecycle::ShuttingDown)
    }

    #[inline]
    pub fn can_control_calls(&self) -> bool {
        self.accepts_user_requests() && self.services.call == ServicePhase::Ready
    }

    #[inline]
    pub fn can_use_contacts(&self) -> bool {
        self.accepts_user_requests() && self.services.database == ServicePhase::Ready
    }

    #[inline]
    pub fn can_use_messaging(&self) -> bool {
        self.accepts_user_requests() && self.services.messaging == ServicePhase::Ready
    }
}

pub(crate) fn request_shutdown(state: &mut AppState) {
    state.lifecycle = AppLifecycle::ShuttingDown;
}

pub(crate) fn recompute_lifecycle(state: &mut AppState) {
    if matches!(state.lifecycle, AppLifecycle::ShuttingDown) {
        return;
    }

    state.lifecycle = derive_lifecycle(&state.services);
}

fn derive_lifecycle(services: &ServicesState) -> AppLifecycle {
    if has_critical_failure(services) {
        return AppLifecycle::Failed;
    }

    if !all_services_terminal(services) {
        return AppLifecycle::Bootstrapping;
    }

    if has_optional_failure(services) { AppLifecycle::Degraded } else { AppLifecycle::Ready }
}

fn all_services_terminal(services: &ServicesState) -> bool {
    [services.database, services.call, services.discovery, services.messaging]
        .into_iter()
        .all(ServicePhase::is_terminal)
}

fn has_critical_failure(services: &ServicesState) -> bool {
    services.call == ServicePhase::Failed
}

fn has_optional_failure(services: &ServicesState) -> bool {
    services.database == ServicePhase::Failed
        || services.discovery == ServicePhase::Failed
        || services.messaging == ServicePhase::Failed
}

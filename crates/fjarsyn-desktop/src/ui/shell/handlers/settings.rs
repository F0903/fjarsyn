use fjarsyn_engine::screen_share;
use iced::Task;

use super::lifecycle;
use crate::ui::{
    message::{self, Message},
    shell::Fjarsyn,
};

pub(in crate::ui::shell) fn handle_settings_msg(
    app: &mut Fjarsyn,
    message: message::Settings,
) -> Task<Message> {
    match message {
        message::Settings::SaveRequested(settings) => save(app, settings, false),
        message::Settings::SaveAndRetryRequested(settings) => save(app, settings, true),
    }
}

fn save(app: &mut Fjarsyn, settings: crate::settings::Settings, retry: bool) -> Task<Message> {
    if let Err(error) = app.state.settings_store.save(&settings) {
        app.state.notify_error(format!("Failed to save settings: {error}"));
        return Task::none();
    }

    let active_engine_settings =
        app.runtime.engine.as_ref().map(|runtime| runtime.active_settings());
    let restart_required =
        requires_restart(app.state.active_power_preference, active_engine_settings, &settings);
    if let Some(runtime) = app.runtime.engine.as_ref() {
        runtime.screen_share().update_config(screen_share::Config::from(&settings.engine));
    }
    app.state.settings = settings;

    if retry {
        let power_change = app.state.settings.power_preference != app.state.active_power_preference;
        if power_change {
            app.state.notify_success(
                "Settings saved. Retrying engine startup; the renderer power preference applies after restarting Fjarsyn.",
            );
        } else {
            app.state.notify_success("Settings saved. Retrying startup.");
        }
        return lifecycle::begin_startup_retry(app);
    }

    if restart_required {
        app.state.notify_success(
            "Settings saved. Power or network changes apply after restarting Fjarsyn.",
        );
    } else {
        app.state.notify_success("Settings saved.");
    }
    Task::none()
}

fn requires_restart(
    active_power_preference: crate::settings::PowerPreference,
    active_engine_settings: Option<&fjarsyn_engine::settings::Settings>,
    desired: &crate::settings::Settings,
) -> bool {
    desired.power_preference != active_power_preference
        || active_engine_settings.is_some_and(|active| active.network != desired.engine.network)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;
    use crate::{
        settings::{PowerPreference, Settings, Store},
        ui::{
            message::Route,
            runtime::RuntimeId,
            screens::Active,
            shell::{Fjarsyn, Lifecycle},
        },
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "fjarsyn-startup-recovery-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            )))
        }

        fn settings_path(&self) -> PathBuf {
            self.0.join("settings.json")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
            let _ = fs::remove_file(&self.0);
        }
    }

    fn startup_failed_app(store: Store) -> Fjarsyn {
        let settings = Settings::default();
        let runtime_id = RuntimeId::next();
        let mut app = Fjarsyn::new(settings, store, runtime_id);
        app.runtime.reject_startup(runtime_id);
        app.state.lifecycle = Lifecycle::StartupFailed("startup failed".into());
        app.active_screen = Active::from_route(Route::Settings, app.state.presentation());
        app
    }

    #[test]
    fn only_power_and_network_changes_require_restart() {
        let active = fjarsyn_engine::settings::Settings::default();
        let mut desired = Settings::default();
        assert!(!requires_restart(PowerPreference::LowPower, Some(&active), &desired));

        desired.engine.capture.record_cursor = !desired.engine.capture.record_cursor;
        assert!(!requires_restart(PowerPreference::LowPower, Some(&active), &desired));

        desired.engine.network.max_depacket_latency_ms += 1;
        assert!(requires_restart(PowerPreference::LowPower, Some(&active), &desired));

        desired.engine.network = active.network.clone();
        desired.power_preference = PowerPreference::HighPerformance;
        assert!(requires_restart(PowerPreference::LowPower, Some(&active), &desired));
    }

    #[test]
    fn successful_recovery_save_persists_before_starting_a_new_runtime() {
        let directory = TestDirectory::new();
        let store = Store::at(directory.settings_path());
        let mut app = startup_failed_app(store.clone());
        let mut desired = Settings::default();
        desired.engine.network.max_depacket_latency_ms += 1;

        drop(save(&mut app, desired.clone(), true));

        assert_eq!(app.state.settings, desired);
        assert_eq!(store.load_or_create().unwrap(), desired);
        assert_eq!(app.state.lifecycle, Lifecycle::Starting);
        assert!(app.runtime.has_pending_startup());
        assert!(app.runtime.engine.is_none());
    }

    #[test]
    fn failed_recovery_save_preserves_state_and_suppresses_retry() {
        let directory = TestDirectory::new();
        fs::write(&directory.0, b"not a directory").unwrap();
        let store = Store::at(directory.settings_path());
        let mut app = startup_failed_app(store);
        let original = app.state.settings.clone();
        let mut desired = original.clone();
        desired.engine.capture.record_cursor = !desired.engine.capture.record_cursor;

        drop(save(&mut app, desired, true));

        assert_eq!(app.state.settings, original);
        assert_eq!(app.state.lifecycle, Lifecycle::StartupFailed("startup failed".into()));
        assert!(!app.runtime.has_pending_startup());
        assert!(
            app.state
                .ui
                .notifications
                .notifications()
                .any(|notification| notification.message.starts_with("Failed to save settings:"))
        );
    }
}

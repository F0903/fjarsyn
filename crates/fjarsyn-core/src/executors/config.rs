use super::AppCommands;
use crate::{
    app::{AppCommand, AppState},
    config::{Config, requires_capture_readback},
};

#[derive(Debug, Clone)]
pub enum ConfigAction {
    SaveRequested(Config),
}

pub fn execute_config_action(state: &mut AppState, action: ConfigAction) -> AppCommands {
    match action {
        ConfigAction::SaveRequested(config) => {
            let config = config.normalized();
            let capture_readback = requires_capture_readback(&config);
            state.config = config;

            smallvec::smallvec![
                AppCommand::SaveConfig {
                    success_message: Some("Settings saved.".into()),
                    error_message: "Unable to save settings".into(),
                },
                AppCommand::ApplyCaptureReadback { enabled: capture_readback },
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executors::test_support::state;

    #[test]
    fn config_save_updates_state_and_emits_persist_commands() {
        let mut state = state();
        let mut config = Config::default();
        config.network.max_depacket_latency = 2_000;

        let commands = execute_config_action(&mut state, ConfigAction::SaveRequested(config));

        assert_eq!(
            state.config.network.max_depacket_latency,
            crate::config::NetworkConfig::MAX_DEPACKET_LATENCY_MS
        );
        assert!(commands.iter().any(|command| matches!(
            command,
            AppCommand::SaveConfig { success_message: Some(message), error_message }
            if message == "Settings saved." && error_message == "Unable to save settings"
        )));
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, AppCommand::ApplyCaptureReadback { .. }))
        );
    }
}

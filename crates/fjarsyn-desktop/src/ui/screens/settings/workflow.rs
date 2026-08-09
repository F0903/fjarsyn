use super::{Screen, tabs};
use crate::{settings::Settings, ui::message::screen::settings::Message};

#[derive(Debug, Clone)]
pub(super) enum Effect {
    SaveSettings(Settings),
    SaveAndRetryStartup(Settings),
}

// The settings workflow keeps UI field mutations local and emits only the work
// that needs runtime access, such as persistence or capture reconfiguration.
pub(super) fn execute_settings_message(
    screen: &mut Screen,
    current_settings: &Settings,
    message: Message,
) -> Vec<Effect> {
    match message {
        Message::TabChanged(tab_id) => {
            screen.active_tab = tabs::get(tab_id);
            Vec::new()
        }
        Message::TranscodingTypeChanged(value) => {
            screen.draft.video.transcoding_type = value;
            Vec::new()
        }
        Message::TargetResolutionChanged(value) => {
            screen.draft.video.target_resolution = value;
            Vec::new()
        }
        Message::TargetFramerateChanged(value) => {
            screen.draft.video.target_framerate = value;
            Vec::new()
        }
        Message::TargetBitrateKbpsChanged(value) => {
            screen.draft.set_target_bitrate_kbps(value);
            Vec::new()
        }
        Message::TargetBitrateKbpsInputChanged(value) => {
            screen.draft.set_target_bitrate_input(value);
            Vec::new()
        }
        Message::RecordCursorChanged(value) => {
            screen.draft.capture.record_cursor = value;
            Vec::new()
        }
        Message::RecordingBorderIndicatorChanged(value) => {
            screen.draft.capture.recording_border_indicator = value;
            Vec::new()
        }
        Message::EnableUiPreviewChanged(value) => {
            screen.draft.capture.enable_ui_preview = value;
            Vec::new()
        }
        Message::MaxDepacketLatencyMsChanged(value) => {
            screen.draft.set_max_depacket_latency_ms(value);
            Vec::new()
        }
        Message::MaxDepacketLatencyMsInputChanged(value) => {
            screen.draft.set_max_depacket_latency_input(value);
            Vec::new()
        }
        Message::SaveSettings => screen
            .draft
            .validate()
            .map(|settings| vec![Effect::SaveSettings(settings)])
            .unwrap_or_default(),
        Message::SaveAndRetryStartup => screen
            .draft
            .validate()
            .map(|settings| vec![Effect::SaveAndRetryStartup(settings)])
            .unwrap_or_default(),
        Message::DiscardSettings => {
            screen.draft = super::SettingsDraft::new(current_settings);
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_recovery_submission_emits_save_and_retry() {
        let current = Settings::default();
        let mut screen = Screen::new(&current);
        screen.draft.capture.record_cursor = !screen.draft.capture.record_cursor;

        let effects = execute_settings_message(&mut screen, &current, Message::SaveAndRetryStartup);

        assert!(matches!(
            effects.as_slice(),
            [Effect::SaveAndRetryStartup(settings)]
                if settings.engine.capture.record_cursor != current.engine.capture.record_cursor
        ));
    }

    #[test]
    fn invalid_recovery_submission_stays_in_the_draft() {
        let current = Settings::default();
        let mut screen = Screen::new(&current);
        execute_settings_message(
            &mut screen,
            &current,
            Message::TargetBitrateKbpsInputChanged(String::new()),
        );

        let effects = execute_settings_message(&mut screen, &current, Message::SaveAndRetryStartup);

        assert!(effects.is_empty());
        assert!(screen.draft.video.target_bitrate_kbps.error().is_some());
    }
}

use fjarsyn_engine::{
    media::{
        codec::TranscodeType,
        video::{Framerate, TargetResolution},
    },
    settings as engine,
};

use crate::settings::{PowerPreference, Settings};

pub(in crate::ui::screens::settings) const MIN_TARGET_BITRATE_KBPS: u32 =
    engine::Video::MIN_TARGET_BITRATE_BPS / 1000;
pub(in crate::ui::screens::settings) const MAX_TARGET_BITRATE_KBPS: u32 =
    engine::Video::MAX_TARGET_BITRATE_BPS / 1000;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SettingsDraft {
    pub(in crate::ui::screens::settings) power_preference: PowerPreference,
    pub(in crate::ui::screens::settings) video: VideoDraft,
    pub(in crate::ui::screens::settings) capture: CaptureDraft,
    pub(in crate::ui::screens::settings) network: NetworkDraft,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::ui::screens::settings) struct VideoDraft {
    pub(in crate::ui::screens::settings) transcoding_type: TranscodeType,
    pub(in crate::ui::screens::settings) target_resolution: TargetResolution,
    pub(in crate::ui::screens::settings) target_framerate: Framerate,
    pub(in crate::ui::screens::settings) target_bitrate_kbps: NumericDraft<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::screens::settings) struct CaptureDraft {
    pub(in crate::ui::screens::settings) record_cursor: bool,
    pub(in crate::ui::screens::settings) recording_border_indicator: bool,
    pub(in crate::ui::screens::settings) enable_ui_preview: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::screens::settings) struct NetworkDraft {
    pub(in crate::ui::screens::settings) max_depacket_latency_ms: NumericDraft<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui::screens::settings) struct NumericDraft<T> {
    text: String,
    last_valid: T,
    error: Option<String>,
}

impl<T> NumericDraft<T> {
    fn new(text: String, last_valid: T) -> Self {
        Self { text, last_valid, error: None }
    }

    pub(in crate::ui::screens::settings) fn text(&self) -> &str {
        &self.text
    }

    pub(in crate::ui::screens::settings) fn last_valid(&self) -> T
    where
        T: Copy,
    {
        self.last_valid
    }

    pub(in crate::ui::screens::settings) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl SettingsDraft {
    pub(super) fn new(settings: &Settings) -> Self {
        let target_bitrate_kbps = settings.engine.video.target_bitrate_bps / 1000;
        Self {
            power_preference: settings.power_preference,
            video: VideoDraft {
                transcoding_type: settings.engine.video.transcoding_type,
                target_resolution: settings.engine.video.target_resolution,
                target_framerate: settings.engine.video.target_framerate,
                target_bitrate_kbps: NumericDraft::new(
                    target_bitrate_kbps.to_string(),
                    target_bitrate_kbps.clamp(MIN_TARGET_BITRATE_KBPS, MAX_TARGET_BITRATE_KBPS),
                ),
            },
            capture: CaptureDraft {
                record_cursor: settings.engine.capture.record_cursor,
                recording_border_indicator: settings.engine.capture.recording_border_indicator,
                enable_ui_preview: settings.engine.capture.enable_ui_preview,
            },
            network: NetworkDraft {
                max_depacket_latency_ms: NumericDraft::new(
                    settings.engine.network.max_depacket_latency_ms.to_string(),
                    settings.engine.network.max_depacket_latency_ms,
                ),
            },
        }
    }

    pub(super) fn set_target_bitrate_kbps(&mut self, value: u32) {
        let value = value.clamp(MIN_TARGET_BITRATE_KBPS, MAX_TARGET_BITRATE_KBPS);
        self.video.target_bitrate_kbps.text = value.to_string();
        self.video.target_bitrate_kbps.last_valid = value;
        self.video.target_bitrate_kbps.error = None;
    }

    pub(super) fn set_target_bitrate_input(&mut self, value: String) {
        self.video.target_bitrate_kbps.text = value;
        self.video.target_bitrate_kbps.error = None;
        if let Ok(value) = parse_target_bitrate_kbps(&self.video.target_bitrate_kbps.text) {
            self.video.target_bitrate_kbps.last_valid = value;
        }
    }

    pub(super) fn set_max_depacket_latency_ms(&mut self, value: u16) {
        let value = value.min(engine::Network::MAX_DEPACKET_LATENCY_MS);
        self.network.max_depacket_latency_ms.text = value.to_string();
        self.network.max_depacket_latency_ms.last_valid = value;
        self.network.max_depacket_latency_ms.error = None;
    }

    pub(super) fn set_max_depacket_latency_input(&mut self, value: String) {
        self.network.max_depacket_latency_ms.text = value;
        self.network.max_depacket_latency_ms.error = None;
        if let Ok(value) = parse_max_depacket_latency_ms(&self.network.max_depacket_latency_ms.text)
        {
            self.network.max_depacket_latency_ms.last_valid = value;
        }
    }

    pub(super) fn validate(&mut self) -> Result<Settings, ()> {
        match self.build() {
            Ok(settings) => {
                self.video.target_bitrate_kbps.error = None;
                self.network.max_depacket_latency_ms.error = None;
                Ok(settings)
            }
            Err(errors) => {
                self.video.target_bitrate_kbps.error = errors.target_bitrate_kbps;
                self.network.max_depacket_latency_ms.error = errors.max_depacket_latency_ms;
                Err(())
            }
        }
    }

    pub(super) fn is_dirty(&self, settings: &Settings) -> bool {
        match self.build() {
            Ok(draft_settings) => draft_settings != *settings,
            Err(_) => true,
        }
    }

    fn build(&self) -> Result<Settings, ValidationErrors> {
        let target_bitrate_bps = parse_target_bitrate_kbps(&self.video.target_bitrate_kbps.text)
            .and_then(|value| {
                value.checked_mul(1000).ok_or_else(|| "Bitrate is too large.".to_owned())
            });
        let max_depacket_latency_ms =
            parse_max_depacket_latency_ms(&self.network.max_depacket_latency_ms.text);

        let errors = ValidationErrors {
            target_bitrate_kbps: target_bitrate_bps.as_ref().err().cloned(),
            max_depacket_latency_ms: max_depacket_latency_ms.as_ref().err().cloned(),
        };
        if errors.has_errors() {
            return Err(errors);
        }

        Ok(Settings {
            power_preference: self.power_preference,
            engine: engine::Settings {
                video: engine::Video {
                    target_bitrate_bps: target_bitrate_bps.expect("validated above"),
                    target_framerate: self.video.target_framerate,
                    target_resolution: self.video.target_resolution,
                    transcoding_type: self.video.transcoding_type,
                },
                capture: engine::Capture {
                    record_cursor: self.capture.record_cursor,
                    recording_border_indicator: self.capture.recording_border_indicator,
                    enable_ui_preview: self.capture.enable_ui_preview,
                },
                network: engine::Network {
                    max_depacket_latency_ms: max_depacket_latency_ms.expect("validated above"),
                },
            },
        })
    }
}

#[derive(Debug)]
struct ValidationErrors {
    target_bitrate_kbps: Option<String>,
    max_depacket_latency_ms: Option<String>,
}

impl ValidationErrors {
    fn has_errors(&self) -> bool {
        self.target_bitrate_kbps.is_some() || self.max_depacket_latency_ms.is_some()
    }
}

fn parse_target_bitrate_kbps(value: &str) -> Result<u32, String> {
    let value =
        value.parse::<u32>().map_err(|_| "Bitrate must be a whole number of kbps.".to_owned())?;
    if !(MIN_TARGET_BITRATE_KBPS..=MAX_TARGET_BITRATE_KBPS).contains(&value) {
        return Err(format!(
            "Bitrate must be between {MIN_TARGET_BITRATE_KBPS} and \
             {MAX_TARGET_BITRATE_KBPS} kbps."
        ));
    }
    Ok(value)
}

fn parse_max_depacket_latency_ms(value: &str) -> Result<u16, String> {
    let value = value
        .parse::<u16>()
        .map_err(|_| "Jitter-buffer latency must be a whole number of milliseconds.".to_owned())?;
    if value > engine::Network::MAX_DEPACKET_LATENCY_MS {
        return Err(format!(
            "Jitter-buffer latency must be between 0 and {} ms.",
            engine::Network::MAX_DEPACKET_LATENCY_MS
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_numeric_input_survives_intermediate_invalid_states() {
        let settings = Settings::default();
        let mut draft = SettingsDraft::new(&settings);
        let last_valid = draft.video.target_bitrate_kbps.last_valid();

        draft.set_target_bitrate_input(String::new());

        assert_eq!(draft.video.target_bitrate_kbps.text(), "");
        assert_eq!(draft.video.target_bitrate_kbps.last_valid(), last_valid);
        assert!(draft.video.target_bitrate_kbps.error().is_none());
        assert!(draft.is_dirty(&settings));
    }

    #[test]
    fn invalid_fields_are_reported_only_when_the_draft_is_validated() {
        let mut draft = SettingsDraft::new(&Settings::default());
        draft.set_target_bitrate_input("99".into());
        draft.set_max_depacket_latency_input("1001".into());

        assert!(draft.video.target_bitrate_kbps.error().is_none());
        assert!(draft.network.max_depacket_latency_ms.error().is_none());
        assert!(draft.validate().is_err());
        assert!(draft.video.target_bitrate_kbps.error().is_some());
        assert!(draft.network.max_depacket_latency_ms.error().is_some());
    }

    #[test]
    fn valid_text_is_converted_to_engine_units() {
        let mut draft = SettingsDraft::new(&Settings::default());
        draft.set_target_bitrate_input("12000".into());
        draft.set_max_depacket_latency_input("75".into());

        let settings = draft.validate().unwrap();

        assert_eq!(settings.engine.video.target_bitrate_bps, 12_000_000);
        assert_eq!(settings.engine.network.max_depacket_latency_ms, 75);
    }

    #[test]
    fn sliders_canonicalize_their_text_fields() {
        let mut draft = SettingsDraft::new(&Settings::default());
        draft.set_target_bitrate_input("invalid".into());
        draft.set_max_depacket_latency_input(String::new());

        draft.set_target_bitrate_kbps(9_000);
        draft.set_max_depacket_latency_ms(80);

        assert_eq!(draft.video.target_bitrate_kbps.text(), "9000");
        assert_eq!(draft.network.max_depacket_latency_ms.text(), "80");
        assert!(draft.validate().is_ok());
    }
}

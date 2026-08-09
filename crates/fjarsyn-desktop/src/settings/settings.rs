use fjarsyn_engine::settings as engine;
use serde::{Deserialize, Serialize};

/// Process-wide GPU preference applied before the desktop renderer starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PowerPreference {
    #[default]
    LowPower,
    HighPerformance,
}

/// Persisted desktop settings. The nested engine settings are deliberately
/// secret-free; local identity material is never part of this document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) power_preference: PowerPreference,
    pub(crate) engine: engine::Settings,
}

impl Settings {
    pub(crate) fn validated(mut self) -> Result<Self, engine::Error> {
        self.engine = self.engine.validated()?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_settings_are_secret_free() {
        let json = serde_json::to_string(&Settings::default()).unwrap();

        assert!(!json.contains("identity"));
        assert!(!json.contains("signing_key"));
        assert!(!json.contains("private_key"));
    }
}

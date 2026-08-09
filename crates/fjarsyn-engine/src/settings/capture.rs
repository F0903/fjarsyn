use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Capture {
    pub record_cursor: bool,
    pub recording_border_indicator: bool,
    pub enable_ui_preview: bool,
}

impl Default for Capture {
    fn default() -> Self {
        Self { record_cursor: true, recording_border_indicator: true, enable_ui_preview: true }
    }
}

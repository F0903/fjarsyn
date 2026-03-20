use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CaptureConfig {
    pub record_cursor: bool,
    pub recording_border_indicator: bool,
    pub enable_ui_preview: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self { record_cursor: true, recording_border_indicator: true, enable_ui_preview: true }
    }
}

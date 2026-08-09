use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("UI error: {0}")]
    Ui(#[from] iced::Error),
    #[error("the operating system did not provide a settings directory")]
    SettingsDirectoryUnavailable,
    #[error("failed to read settings from {path}: {source}")]
    SettingsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("settings file {path} exceeds the {max_bytes} byte limit")]
    SettingsTooLarge { path: PathBuf, max_bytes: usize },
    #[error("failed to parse settings from {path}: {source}")]
    SettingsParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("settings in {path} are invalid: {source}")]
    SettingsValidation {
        path: PathBuf,
        #[source]
        source: fjarsyn_engine::settings::Error,
    },
    #[error("failed to serialize settings: {0}")]
    SettingsSerialize(#[source] serde_json::Error),
    #[error("failed to write settings to {path}: {source}")]
    SettingsWrite {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

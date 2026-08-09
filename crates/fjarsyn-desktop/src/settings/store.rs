use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
#[cfg(target_os = "windows")]
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};

use super::Settings;
use crate::{Error, Result};

const MAX_SETTINGS_BYTES: usize = 1024 * 1024;

/// Atomic persistence for the desktop's secret-free settings document.
#[derive(Debug, Clone)]
pub(crate) struct Store {
    path: PathBuf,
}

impl Store {
    pub(crate) fn system() -> Result<Self> {
        let project_dirs =
            ProjectDirs::from("", "", "fjarsyn").ok_or(Error::SettingsDirectoryUnavailable)?;
        Ok(Self { path: project_dirs.config_dir().join("settings.json") })
    }

    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn load_or_create(&self) -> Result<Settings> {
        match self.read_bounded()? {
            Some(bytes) => {
                let settings = serde_json::from_slice::<Settings>(&bytes)
                    .map_err(|source| Error::SettingsParse { path: self.path.clone(), source })?;
                settings
                    .validated()
                    .map_err(|source| Error::SettingsValidation { path: self.path.clone(), source })
            }
            None => {
                let settings = Settings::default().validated().map_err(|source| {
                    Error::SettingsValidation { path: self.path.clone(), source }
                })?;
                self.save(&settings)?;
                Ok(settings)
            }
        }
    }

    pub(crate) fn save(&self, settings: &Settings) -> Result<()> {
        let settings = settings
            .clone()
            .validated()
            .map_err(|source| Error::SettingsValidation { path: self.path.clone(), source })?;
        let bytes = serde_json::to_vec_pretty(&settings).map_err(Error::SettingsSerialize)?;
        write_atomically(&self.path, &bytes)
            .map_err(|source| Error::SettingsWrite { path: self.path.clone(), source })
    }

    fn read_bounded(&self) -> Result<Option<Vec<u8>>> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::SettingsRead { path: self.path.clone(), source });
            }
        };
        let file_len = file
            .metadata()
            .map_err(|source| Error::SettingsRead { path: self.path.clone(), source })?
            .len();
        if file_len > MAX_SETTINGS_BYTES as u64 {
            return Err(Error::SettingsTooLarge {
                path: self.path.clone(),
                max_bytes: MAX_SETTINGS_BYTES,
            });
        }

        let mut bytes = Vec::with_capacity(file_len as usize);
        file.take(MAX_SETTINGS_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| Error::SettingsRead { path: self.path.clone(), source })?;
        if bytes.len() > MAX_SETTINGS_BYTES {
            return Err(Error::SettingsTooLarge {
                path: self.path.clone(),
                max_bytes: MAX_SETTINGS_BYTES,
            });
        }
        Ok(Some(bytes))
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomically_with(path, bytes, replace_file)
}

fn write_atomically_with(
    path: &Path,
    bytes: &[u8],
    replace: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "settings path has no parent directory")
    })?;
    fs::create_dir_all(parent)?;

    let temp_path = temporary_path(path)?;
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace(&temp_path, path)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;

    let source = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let destination = destination.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: Both path buffers are NUL-terminated and remain alive for the
    // duration of the call. They identify files in the same directory.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(io::Error::other)
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

fn temporary_path(path: &Path) -> io::Result<PathBuf> {
    let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "settings path has no UTF-8 file name")
    })?;
    Ok(path.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4())))
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "fjarsyn-desktop-settings-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            Self(path)
        }

        fn settings_path(&self) -> PathBuf {
            self.0.join("settings.json")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_settings_are_created_and_round_trip() {
        let directory = TestDirectory::new();
        let store = Store::at(directory.settings_path());

        let mut settings = store.load_or_create().unwrap();
        settings.power_preference = super::super::PowerPreference::HighPerformance;
        settings.engine.capture.enable_ui_preview = false;
        store.save(&settings).unwrap();

        assert_eq!(store.load_or_create().unwrap(), settings);
    }

    #[test]
    fn replacing_settings_leaves_no_temporary_file() {
        let directory = TestDirectory::new();
        let store = Store::at(directory.settings_path());
        store.save(&Settings::default()).unwrap();

        let mut replacement = Settings::default();
        replacement.engine.capture.record_cursor = false;
        store.save(&replacement).unwrap();

        assert_eq!(store.load_or_create().unwrap(), replacement);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn malformed_settings_are_reported_without_being_replaced() {
        let directory = TestDirectory::new();
        fs::create_dir_all(&directory.0).unwrap();
        let path = directory.settings_path();
        fs::write(&path, b"not json").unwrap();
        let store = Store::at(path.clone());

        assert!(matches!(store.load_or_create(), Err(Error::SettingsParse { .. })));
        assert_eq!(fs::read(path).unwrap(), b"not json");
    }

    #[test]
    fn invalid_engine_settings_are_rejected_without_being_rewritten() {
        let directory = TestDirectory::new();
        fs::create_dir_all(&directory.0).unwrap();
        let path = directory.settings_path();
        let invalid = br#"{"engine":{"video":{"target_bitrate_bps":0}}}"#;
        fs::write(&path, invalid).unwrap();
        let store = Store::at(path.clone());

        assert!(matches!(store.load_or_create(), Err(Error::SettingsValidation { .. })));
        assert_eq!(fs::read(path).unwrap(), invalid);
    }

    #[test]
    fn invalid_settings_cannot_replace_a_valid_document() {
        let directory = TestDirectory::new();
        let path = directory.settings_path();
        let store = Store::at(path.clone());
        store.save(&Settings::default()).unwrap();
        let previous = fs::read(&path).unwrap();
        let mut invalid = Settings::default();
        invalid.engine.video.target_bitrate_bps = 0;

        assert!(matches!(store.save(&invalid), Err(Error::SettingsValidation { .. })));
        assert_eq!(fs::read(path).unwrap(), previous);
    }

    #[test]
    fn settings_that_cannot_round_trip_through_the_kbps_ui_are_rejected() {
        let directory = TestDirectory::new();
        let path = directory.settings_path();
        let store = Store::at(path.clone());
        store.save(&Settings::default()).unwrap();
        let previous = fs::read(&path).unwrap();
        let mut invalid = Settings::default();
        invalid.engine.video.target_bitrate_bps += 1;

        assert!(matches!(store.save(&invalid), Err(Error::SettingsValidation { .. })));
        assert_eq!(fs::read(path).unwrap(), previous);
    }

    #[test]
    fn oversized_settings_are_rejected_before_reading_the_document() {
        let directory = TestDirectory::new();
        fs::create_dir_all(&directory.0).unwrap();
        let path = directory.settings_path();
        File::create(&path).unwrap().set_len(MAX_SETTINGS_BYTES as u64 + 1).unwrap();
        let store = Store::at(path.clone());

        assert!(matches!(store.load_or_create(), Err(Error::SettingsTooLarge { .. })));
        assert_eq!(fs::metadata(path).unwrap().len(), MAX_SETTINGS_BYTES as u64 + 1);
    }

    #[test]
    fn failed_atomic_replace_preserves_the_previous_document() {
        let directory = TestDirectory::new();
        fs::create_dir_all(&directory.0).unwrap();
        let path = directory.settings_path();
        fs::write(&path, b"previous").unwrap();

        let result = write_atomically_with(&path, b"replacement", |_, _| {
            Err(io::Error::other("injected replace failure"))
        });

        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"previous");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }
}

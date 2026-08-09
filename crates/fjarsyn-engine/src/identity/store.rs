use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{LocalIdentity, LocalPeerIdentity, PeerId, StoredIdentityKeypair};
use crate::paths::{CONFIG_DIR, DATA_DIR};

const FILE_HEADER: &[u8] = b"FJARSYN-IDENTITY\x01";
const MAX_PROTECTED_FILE_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StoredIdentity {
    peer_id: PeerId,
    keypair: StoredIdentityKeypair,
}

impl StoredIdentity {
    fn from_runtime(identity: &LocalIdentity) -> Self {
        Self {
            peer_id: identity.peer_id().clone(),
            keypair: identity.signing_identity().to_stored(),
        }
    }

    fn into_runtime(self) -> Result<LocalIdentity, Error> {
        let signing_identity = LocalPeerIdentity::from_stored(&self.keypair)?;
        Ok(LocalIdentity::from_parts(self.peer_id, signing_identity))
    }
}

/// Private owner of the stable, per-user local identity record.
#[derive(Debug)]
pub(crate) struct Store {
    path: PathBuf,
    obsolete_config_path: PathBuf,
}

impl Store {
    pub(crate) fn user() -> Self {
        Self {
            path: DATA_DIR.join("identity.bin"),
            obsolete_config_path: CONFIG_DIR.join("config.json"),
        }
    }

    #[cfg(test)]
    fn new(path: PathBuf, obsolete_config_path: PathBuf) -> Self {
        Self { path, obsolete_config_path }
    }

    pub(crate) fn load_or_create(&self) -> Result<LocalIdentity, Error> {
        self.load_or_create_with(|| {
            let peer_id = PeerId::new(uuid::Uuid::new_v4().to_string())?;
            Ok(LocalIdentity::generate(peer_id))
        })
    }

    fn load_or_create_with(
        &self,
        create_identity: impl FnOnce() -> Result<LocalIdentity, Error>,
    ) -> Result<LocalIdentity, Error> {
        let identity = match self.load() {
            Ok(identity) => identity,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                let candidate = create_identity()?;
                match self.publish(&candidate)? {
                    PublishOutcome::Installed => candidate,
                    PublishOutcome::AlreadyExists => self.load()?,
                }
            }
            Err(error) => return Err(error),
        };

        // The old mixed settings/identity document may contain the signing key
        // in plaintext. It is deliberately neither parsed nor migrated. Only
        // remove it after a protected identity is durably available, and fail
        // startup if removal cannot be confirmed.
        self.remove_obsolete_config()?;
        Ok(identity)
    }

    fn load(&self) -> Result<LocalIdentity, Error> {
        let file = read_bounded(&self.path, MAX_PROTECTED_FILE_BYTES)?;
        let ciphertext = file.strip_prefix(FILE_HEADER).ok_or(Error::UnsupportedFormat)?;
        if ciphertext.is_empty() {
            return Err(Error::UnsupportedFormat);
        }

        #[cfg(target_os = "windows")]
        let mut plaintext = super::protection::unprotect(ciphertext).map_err(Error::Unprotect)?;
        #[cfg(not(target_os = "windows"))]
        let plaintext = return Err(Error::UnsupportedPlatform);

        let decoded = serde_json::from_slice::<StoredIdentity>(&plaintext);
        plaintext.fill(0);
        decoded?.into_runtime()
    }

    fn publish(&self, identity: &LocalIdentity) -> Result<PublishOutcome, Error> {
        let stored = StoredIdentity::from_runtime(identity);
        let mut plaintext = serde_json::to_vec(&stored)?;
        drop(stored);

        #[cfg(target_os = "windows")]
        let protected = super::protection::protect(&plaintext);
        plaintext.fill(0);
        #[cfg(target_os = "windows")]
        let ciphertext = protected.map_err(Error::Protect)?;
        #[cfg(not(target_os = "windows"))]
        let ciphertext = return Err(Error::UnsupportedPlatform);

        if FILE_HEADER.len().saturating_add(ciphertext.len()) > MAX_PROTECTED_FILE_BYTES {
            return Err(Error::IdentityTooLarge);
        }
        let mut file = Vec::with_capacity(FILE_HEADER.len() + ciphertext.len());
        file.extend_from_slice(FILE_HEADER);
        file.extend_from_slice(&ciphertext);
        write_new_atomically(&self.path, &file)
    }

    fn remove_obsolete_config(&self) -> Result<(), Error> {
        match fs::remove_file(&self.obsolete_config_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::RemoveObsoleteConfig(error)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("failed to access local identity file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to encode or decode local identity: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid local identity: {0}")]
    Identity(#[from] super::Error),
    #[error("invalid local peer identifier: {0}")]
    PeerId(#[from] super::PeerIdError),
    #[cfg(target_os = "windows")]
    #[error("failed to protect local identity with Windows DPAPI: {0}")]
    Protect(#[source] windows::core::Error),
    #[cfg(target_os = "windows")]
    #[error("failed to unprotect local identity with Windows DPAPI: {0}")]
    Unprotect(#[source] windows::core::Error),
    #[cfg(not(target_os = "windows"))]
    #[error("protected local identity storage is only supported on Windows")]
    UnsupportedPlatform,
    #[error("local identity file has an unsupported format")]
    UnsupportedFormat,
    #[error("local identity exceeds its storage limit")]
    IdentityTooLarge,
    #[error("failed to remove obsolete plaintext configuration: {0}")]
    RemoveObsoleteConfig(#[source] std::io::Error),
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, Error> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let max_length = u64::try_from(limit).map_err(|_| Error::IdentityTooLarge)?;
    if metadata.len() > max_length {
        return Err(Error::IdentityTooLarge);
    }

    // Metadata is only an early rejection: the file can grow between that
    // check and the read. Taking one byte beyond the limit makes the bound
    // race-safe without allocating from attacker-controlled metadata.
    let capacity = usize::try_from(metadata.len()).map_err(|_| Error::IdentityTooLarge)?;
    let mut contents = Vec::with_capacity(capacity);
    file.take(max_length.saturating_add(1)).read_to_end(&mut contents)?;
    if contents.len() > limit {
        return Err(Error::IdentityTooLarge);
    }
    Ok(contents)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishOutcome {
    Installed,
    AlreadyExists,
}

fn write_new_atomically(path: &Path, contents: &[u8]) -> Result<PublishOutcome, Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("identity");
    let temporary = path.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        install_file_if_absent(&temporary, path)
    })();

    match result {
        Ok(PublishOutcome::Installed) => Ok(PublishOutcome::Installed),
        Ok(PublishOutcome::AlreadyExists) => {
            fs::remove_file(&temporary)?;
            Ok(PublishOutcome::AlreadyExists)
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(Error::Io(error))
        }
    }
}

#[cfg(target_os = "windows")]
fn install_file_if_absent(source: &Path, destination: &Path) -> std::io::Result<PublishOutcome> {
    use std::os::windows::ffi::OsStrExt;

    use windows::{
        Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, WIN32_ERROR},
            Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW},
        },
        core::PCWSTR,
    };

    let source = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let destination = destination.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: both paths are live, NUL-terminated UTF-16 buffers for the
    // duration of the call. Omitting MOVEFILE_REPLACE_EXISTING makes this an
    // immutable install: exactly one concurrent first writer can succeed.
    let result = unsafe {
        MoveFileExW(PCWSTR(source.as_ptr()), PCWSTR(destination.as_ptr()), MOVEFILE_WRITE_THROUGH)
    };
    match result {
        Ok(()) => Ok(PublishOutcome::Installed),
        Err(error)
            if WIN32_ERROR::from_error(&error)
                .is_some_and(|code| code == ERROR_ALREADY_EXISTS || code == ERROR_FILE_EXISTS) =>
        {
            Ok(PublishOutcome::AlreadyExists)
        }
        Err(error) => {
            let io_error = WIN32_ERROR::from_error(&error)
                .map(|code| std::io::Error::from_raw_os_error(code.0 as i32))
                .unwrap_or_else(|| std::io::Error::other(error));
            Err(io_error)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn install_file_if_absent(source: &Path, destination: &Path) -> std::io::Result<PublishOutcome> {
    match fs::hard_link(source, destination) {
        Ok(()) => {
            fs::remove_file(source)?;
            Ok(PublishOutcome::Installed)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Ok(PublishOutcome::AlreadyExists)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("fjarsyn-identity-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn store(&self) -> Store {
            Store::new(self.0.join("identity.bin"), self.0.join("config.json"))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn protected_identity_round_trips_without_plaintext_key_material() {
        let directory = TestDirectory::new();
        let store = directory.store();
        let created = store.load_or_create().unwrap();
        let private_key = created.signing_identity().to_stored();
        let loaded = store.load_or_create().unwrap();

        assert_eq!(loaded.peer_id(), created.peer_id());
        assert_eq!(loaded.public_key_base64(), created.public_key_base64());
        let stored = fs::read(directory.0.join("identity.bin")).unwrap();
        assert!(stored.starts_with(FILE_HEADER));
        assert!(
            !stored
                .windows(private_key.private_key.len())
                .any(|window| window == private_key.private_key.as_bytes())
        );
    }

    #[test]
    fn corrupt_existing_identity_fails_instead_of_rotating_it() {
        let directory = TestDirectory::new();
        let path = directory.0.join("identity.bin");
        fs::write(&path, b"not an identity").unwrap();

        assert!(matches!(directory.store().load_or_create(), Err(Error::UnsupportedFormat)));
        assert_eq!(fs::read(path).unwrap(), b"not an identity");
    }

    #[test]
    fn oversized_identity_is_rejected_before_parsing() {
        let directory = TestDirectory::new();
        let path = directory.0.join("identity.bin");
        fs::write(&path, vec![0; MAX_PROTECTED_FILE_BYTES + 1]).unwrap();

        assert!(matches!(directory.store().load_or_create(), Err(Error::IdentityTooLarge)));
    }

    #[test]
    fn obsolete_plaintext_config_is_removed_only_after_protected_identity_exists() {
        let directory = TestDirectory::new();
        let obsolete = directory.0.join("config.json");
        fs::write(&obsolete, br#"{"identity":"plaintext"}"#).unwrap();

        directory.store().load_or_create().unwrap();

        assert!(directory.0.join("identity.bin").is_file());
        assert!(!obsolete.exists());
    }

    #[test]
    fn obsolete_config_removal_failure_fails_after_identity_is_durable() {
        let directory = TestDirectory::new();
        let obsolete = directory.0.join("config.json");
        fs::create_dir(&obsolete).unwrap();

        assert!(matches!(directory.store().load_or_create(), Err(Error::RemoveObsoleteConfig(_))));
        assert!(directory.0.join("identity.bin").is_file());
    }

    #[test]
    fn concurrent_first_startups_converge_on_one_immutable_identity() {
        use std::sync::{Arc, Barrier};

        const CALLERS: usize = 16;

        let directory = TestDirectory::new();
        let path = directory.0.join("identity.bin");
        let obsolete = directory.0.join("config.json");
        let barrier = Arc::new(Barrier::new(CALLERS));
        let callers = (0..CALLERS)
            .map(|index| {
                let path = path.clone();
                let obsolete = obsolete.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let candidate_peer = PeerId::new(format!("candidate-{index}")).unwrap();
                    let candidate = LocalIdentity::generate(candidate_peer.clone());
                    let identity = Store::new(path, obsolete)
                        .load_or_create_with(|| {
                            barrier.wait();
                            Ok(candidate)
                        })
                        .unwrap();
                    (candidate_peer, identity.peer_id().clone(), identity.public_key_base64())
                })
            })
            .collect::<Vec<_>>();
        let resolved = callers.into_iter().map(|caller| caller.join().unwrap()).collect::<Vec<_>>();

        let (_, expected_peer, expected_key) = &resolved[0];
        assert!(
            resolved.iter().all(|(_, peer, key)| { peer == expected_peer && key == expected_key })
        );
        assert_eq!(
            resolved.iter().filter(|(candidate, _, _)| candidate == expected_peer).count(),
            1,
            "the published identity must belong to exactly one first writer"
        );

        let entries = fs::read_dir(&directory.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, [std::ffi::OsString::from("identity.bin")]);
    }
}

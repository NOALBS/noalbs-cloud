use std::{collections::HashSet, fs, io::Write, path::PathBuf};

use directories::BaseDirs;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("could not find the user configuration directory")]
    NoConfigDir,
    #[error("credential keyring operation failed: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("credential file I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct TokenStore {
    root: PathBuf,
}

impl TokenStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_store() -> Result<Self, CredentialError> {
        let dirs = BaseDirs::new().ok_or(CredentialError::NoConfigDir)?;
        Ok(Self::new(dirs.config_dir().join("noalbs-connect")))
    }

    pub fn set(&self, setup: Uuid, token: &str) -> Result<(), CredentialError> {
        if let Ok(entry) = keyring::Entry::new("com.noalbs.connect", &setup.to_string())
            && entry.set_password(token).is_ok()
        {
            self.keyring_write_succeeded(setup);
            return Ok(());
        }
        self.write_fallback(setup, token)
    }

    pub fn get(&self, setup: Uuid) -> Result<Option<String>, CredentialError> {
        if let Ok(entry) = keyring::Entry::new("com.noalbs.connect", &setup.to_string())
            && let Ok(token) = entry.get_password()
        {
            return Ok(Some(token));
        }
        let path = self.fallback_path(setup);
        match fs::read_to_string(path) {
            Ok(token) => Ok(Some(token)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn delete(&self, setup: Uuid) -> Result<(), CredentialError> {
        let keyring_result = keyring::Entry::new("com.noalbs.connect", &setup.to_string())
            .and_then(|entry| entry.delete_credential());
        let fallback_result = self.remove_fallback(setup);
        keyring_delete_result(keyring_result)?;
        fallback_result
    }

    pub fn remove_orphan_fallback_tokens(
        &self,
        active_setups: &HashSet<Uuid>,
    ) -> Result<(), CredentialError> {
        let entries = match fs::read_dir(self.root.join("tokens")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(setup_id) = entry
                .file_name()
                .to_str()
                .and_then(|name| Uuid::parse_str(name).ok())
            else {
                continue;
            };
            if !active_setups.contains(&setup_id) {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    fn remove_fallback(&self, setup: Uuid) -> Result<(), CredentialError> {
        match fs::remove_file(self.fallback_path(setup)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn fallback_path(&self, setup: Uuid) -> PathBuf {
        self.root.join("tokens").join(setup.to_string())
    }

    fn write_fallback(&self, setup: Uuid, token: &str) -> Result<(), CredentialError> {
        let path = self.fallback_path(setup);
        let parent = path.parent().expect("token path has a parent");
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
        }
        #[cfg(not(unix))]
        {
            let mut file = fs::File::create(&path)?;
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
        }
        Ok(())
    }

    fn keyring_write_succeeded(&self, setup: Uuid) {
        if let Err(error) = self.remove_fallback(setup) {
            tracing::warn!(
                setup_id = %setup,
                error = %error,
                "Failed to remove stale fallback token after storing token in the keyring"
            );
        }
    }
}

fn keyring_delete_result(result: Result<(), keyring::Error>) -> Result<(), CredentialError> {
    match result {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tempfile::tempdir;

    use super::{CredentialError, TokenStore, keyring_delete_result};
    use uuid::Uuid;

    #[test]
    fn fallback_token_cleanup_removes_stale_tokens() {
        let directory = tempdir().unwrap();
        let store = TokenStore::new(directory.path().to_owned());
        let setup = Uuid::new_v4();
        store.write_fallback(setup, "stale-token").unwrap();
        let fallback = store.fallback_path(setup);

        store.keyring_write_succeeded(setup);

        assert!(!fallback.exists());
        store.write_fallback(setup, "fallback-token").unwrap();
        store.remove_fallback(setup).unwrap();
        assert!(!fallback.exists());
    }

    #[test]
    fn keyring_delete_treats_absent_entries_as_success() {
        assert!(keyring_delete_result(Err(keyring::Error::NoEntry)).is_ok());
    }

    #[test]
    fn keyring_delete_returns_errors_other_than_no_entry() {
        let error =
            keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("keyring locked")));

        assert!(matches!(
            keyring_delete_result(Err(error)),
            Err(CredentialError::Keyring(_))
        ));
    }

    #[test]
    fn fallback_cleanup_failure_after_keyring_write_is_non_fatal() {
        let directory = tempdir().unwrap();
        let store = TokenStore::new(directory.path().to_owned());
        let setup = Uuid::new_v4();
        let fallback = store.fallback_path(setup);
        std::fs::create_dir_all(fallback.parent().unwrap()).unwrap();
        std::fs::create_dir(&fallback).unwrap();

        store.keyring_write_succeeded(setup);

        assert!(fallback.is_dir());
    }

    #[test]
    fn orphaned_fallback_tokens_are_removed() {
        let directory = tempdir().unwrap();
        let store = TokenStore::new(directory.path().to_owned());
        let active_setup = Uuid::new_v4();
        let orphaned_setup = Uuid::new_v4();
        store.write_fallback(active_setup, "active-token").unwrap();
        store
            .write_fallback(orphaned_setup, "orphaned-token")
            .unwrap();

        store
            .remove_orphan_fallback_tokens(&HashSet::from([active_setup]))
            .unwrap();

        assert_eq!(
            store.get(active_setup).unwrap().as_deref(),
            Some("active-token")
        );
        assert_eq!(store.get(orphaned_setup).unwrap(), None);
    }
}

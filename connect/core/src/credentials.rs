use std::{fs, io::Write, path::PathBuf};

use directories::BaseDirs;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("could not find the user configuration directory")]
    NoConfigDir,
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
        if let Ok(entry) = keyring::Entry::new("com.noalbs.connect", &setup.to_string()) {
            let _ = entry.delete_credential();
        }
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
}

//! The long-term identity of this device: one Ed25519 key pair.
//!
//! The public key is the user's address. There is no account, phone number or
//! e-mail behind it.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use iroh::{EndpointId, SecretKey};

/// The secret key of this device.
///
/// Milestone 1 keeps the key in a plain file readable only by the owner.
/// Moving it into the operating system keystore is planned for milestone 2.
pub struct Identity {
    secret: SecretKey,
}

impl Identity {
    /// Generates a new identity and writes it to `path`.
    ///
    /// Refuses to overwrite an existing file, because that would destroy the
    /// previous identity for good.
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("cannot create directory {}", dir.display()))?;
        }
        let secret = SecretKey::generate();
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .with_context(|| format!("cannot create identity file {}", path.display()))?;
        file.write_all(&secret.to_bytes())?;
        file.sync_all()?;
        Ok(Self { secret })
    }

    /// Loads the identity stored at `path`.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)
            .with_context(|| format!("cannot read identity file {}", path.display()))?;
        let Ok(bytes) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            bail!("identity file {} is damaged", path.display());
        };
        Ok(Self {
            secret: SecretKey::from_bytes(&bytes),
        })
    }

    /// Returns `true` if an identity file exists at `path`.
    pub fn exists(path: &Path) -> bool {
        path.is_file()
    }

    /// The public identifier other people use to add this device as a contact.
    pub fn id(&self) -> EndpointId {
        self.secret.public()
    }

    pub(crate) fn secret_key(&self) -> SecretKey {
        self.secret.clone()
    }

    /// The raw secret key, for an encrypted archive. Never store it in clear.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }

    /// Replaces the identity at `path` with the secret key from an archive.
    /// The identity that was there is gone for good.
    pub fn restore(path: &Path, secret: [u8; 32]) -> Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("cannot create directory {}", dir.display()))?;
        }
        let tmp = path.with_extension("key.tmp");
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .with_context(|| format!("cannot write identity file {}", tmp.display()))?;
        file.write_all(&secret)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
            .with_context(|| format!("cannot write identity file {}", path.display()))?;
        Ok(Self {
            secret: SecretKey::from_bytes(&secret),
        })
    }
}

/// The conventional location of the identity file inside a data directory.
pub fn default_path(data_dir: &Path) -> PathBuf {
    data_dir.join("identity.key")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_load_gives_the_same_id() {
        let dir = std::env::temp_dir().join(format!("komunikator-id-{}", rand::random::<u64>()));
        let path = default_path(&dir);
        let created = Identity::create(&path).unwrap();
        let loaded = Identity::load(&path).unwrap();
        assert_eq!(created.id(), loaded.id());
        assert!(Identity::create(&path).is_err(), "must not overwrite");
        fs::remove_dir_all(dir).unwrap();
    }
}

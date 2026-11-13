//! The address book: a local alias for each known public key.
//!
//! Only people in the address book can connect to this device.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use iroh::EndpointId;

/// The address book, stored as a JSON file.
#[derive(Debug)]
pub struct Contacts {
    path: PathBuf,
    by_alias: BTreeMap<String, EndpointId>,
}

impl Contacts {
    /// Loads the address book from `path`. A missing file is an empty book.
    pub fn load(path: &Path) -> Result<Self> {
        let by_alias = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("contacts file {} is damaged", path.display()))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(err) => {
                return Err(err).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        Ok(Self {
            path: path.to_owned(),
            by_alias,
        })
    }

    /// Adds a contact and saves the book.
    pub fn add(&mut self, alias: &str, id: EndpointId) -> Result<()> {
        let alias = alias.trim();
        if alias.is_empty() || alias.chars().any(char::is_whitespace) {
            bail!("alias must be a single word");
        }
        if self.by_alias.contains_key(alias) {
            bail!("alias `{alias}` is already taken");
        }
        if let Some(existing) = self.alias_of(&id) {
            bail!("this key is already saved as `{existing}`");
        }
        self.by_alias.insert(alias.to_owned(), id);
        self.save()
    }

    /// Removes a contact and saves the book. Returns `false` if it was unknown.
    pub fn remove(&mut self, alias: &str) -> Result<bool> {
        let removed = self.by_alias.remove(alias).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// Gives the contact `alias` a new key, after that person reinstalled the
    /// app and got a new identity. Returns the old key.
    pub fn replace_id(&mut self, alias: &str, id: EndpointId) -> Result<EndpointId> {
        let Some(old) = self.by_alias.get(alias).copied() else {
            bail!("unknown contact `{alias}`");
        };
        if old == id {
            bail!("this contact already has this key");
        }
        if let Some(existing) = self.alias_of(&id) {
            bail!("this key is already saved as `{existing}`");
        }
        self.by_alias.insert(alias.to_owned(), id);
        self.save()?;
        Ok(old)
    }

    /// Looks up the key saved under `alias`.
    pub fn get(&self, alias: &str) -> Option<EndpointId> {
        self.by_alias.get(alias).copied()
    }

    /// Looks up the alias saved for `id`.
    pub fn alias_of(&self, id: &EndpointId) -> Option<&str> {
        self.by_alias
            .iter()
            .find(|(_, saved)| *saved == id)
            .map(|(alias, _)| alias.as_str())
    }

    /// All contacts, sorted by alias.
    pub fn iter(&self) -> impl Iterator<Item = (&str, EndpointId)> {
        self.by_alias
            .iter()
            .map(|(alias, id)| (alias.as_str(), *id))
    }

    /// Writes to a temporary file first, so a crash cannot truncate the book.
    fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.by_alias)?)?;
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("cannot write {}", self.path.display()))
    }
}

/// The conventional location of the address book inside a data directory.
pub fn default_path(data_dir: &Path) -> PathBuf {
    data_dir.join("contacts.json")
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    #[test]
    fn add_save_reload_remove() {
        let dir = std::env::temp_dir().join(format!("komunikator-ab-{}", rand::random::<u64>()));
        let path = default_path(&dir);
        let ala = SecretKey::generate().public();

        let mut book = Contacts::load(&path).unwrap();
        book.add("ala", ala).unwrap();
        assert!(book.add("ala", SecretKey::generate().public()).is_err());
        assert!(book.add("ala2", ala).is_err());
        assert!(
            book.add("dwa slowa", SecretKey::generate().public())
                .is_err()
        );

        let mut book = Contacts::load(&path).unwrap();
        assert_eq!(book.get("ala"), Some(ala));
        assert_eq!(book.alias_of(&ala), Some("ala"));
        let new_key = SecretKey::generate().public();
        assert_eq!(book.replace_id("ala", new_key).unwrap(), ala);
        assert!(book.replace_id("ala", new_key).is_err());
        assert!(book.replace_id("nikt", ala).is_err());
        assert_eq!(Contacts::load(&path).unwrap().get("ala"), Some(new_key));
        assert!(book.remove("ala").unwrap());
        assert!(!book.remove("ala").unwrap());
        fs::remove_dir_all(dir).unwrap();
    }
}

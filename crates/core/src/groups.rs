//! Group chats this device takes part in, as their owners last described
//! them.
//!
//! A group has no server: every message goes straight from its author to
//! each member. Members who are not contacts may connect too, but only for
//! the group's sake (see `komunikator-ffi`).

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use iroh::EndpointId;

use crate::protocol::{GroupId, GroupInfo};

/// The groups, stored as a JSON file.
#[derive(Debug)]
pub struct Groups {
    path: PathBuf,
    by_id: BTreeMap<String, GroupInfo>,
}

impl Groups {
    /// Loads the groups from `path`. A missing file means no groups.
    pub fn load(path: &Path) -> Result<Self> {
        let by_id = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("groups file {} is damaged", path.display()))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(err) => {
                return Err(err).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        Ok(Self {
            path: path.to_owned(),
            by_id,
        })
    }

    pub fn get(&self, id: &GroupId) -> Option<&GroupInfo> {
        self.by_id.get(&id.to_string())
    }

    pub fn iter(&self) -> impl Iterator<Item = &GroupInfo> {
        self.by_id.values()
    }

    /// Stores `info` if it is new or newer than what is stored. Returns
    /// `true` if it was stored.
    pub fn accept(&mut self, info: GroupInfo) -> Result<bool> {
        let key = info.id.to_string();
        if self
            .by_id
            .get(&key)
            .is_some_and(|known| known.version >= info.version)
        {
            return Ok(false);
        }
        self.by_id.insert(key, info);
        self.save()?;
        Ok(true)
    }

    /// Forgets a group. Returns what was stored.
    pub fn remove(&mut self, id: &GroupId) -> Result<Option<GroupInfo>> {
        let removed = self.by_id.remove(&id.to_string());
        if removed.is_some() {
            self.save()?;
        }
        Ok(removed)
    }

    /// Everyone in any group, except `me`.
    pub fn all_members(&self, me: &EndpointId) -> Vec<EndpointId> {
        let mut all: Vec<EndpointId> = self
            .by_id
            .values()
            .flat_map(|info| info.others(me))
            .collect();
        all.sort_by_key(|id| *id.as_bytes());
        all.dedup();
        all
    }

    /// Returns `true` if `peer` shares a group with this device.
    pub fn knows(&self, peer: &EndpointId) -> bool {
        self.by_id.values().any(|info| info.has(peer))
    }

    fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.by_id)?)?;
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("cannot write {}", self.path.display()))
    }
}

/// The conventional location of the groups file inside a data directory.
pub fn default_path(data_dir: &Path) -> PathBuf {
    data_dir.join("groups.json")
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;
    use crate::protocol::Member;

    #[test]
    fn only_newer_versions_replace_a_group() {
        let dir = std::env::temp_dir().join(format!("komunikator-gr-{}", rand::random::<u64>()));
        let path = default_path(&dir);
        let me = SecretKey::generate().public();
        let ola = SecretKey::generate().public();
        let info = GroupInfo {
            id: GroupId::random(),
            owner: *me.as_bytes(),
            version: 1,
            name: "Rodzina".into(),
            members: vec![
                Member {
                    id: *me.as_bytes(),
                    name: "Ja".into(),
                },
                Member {
                    id: *ola.as_bytes(),
                    name: "Ola".into(),
                },
            ],
        };
        let mut groups = Groups::load(&path).unwrap();
        assert!(groups.accept(info.clone()).unwrap());
        assert!(!groups.accept(info.clone()).unwrap());
        let renamed = GroupInfo {
            version: 2,
            name: "Rodzinka".into(),
            ..info.clone()
        };
        assert!(groups.accept(renamed).unwrap());

        let groups = Groups::load(&path).unwrap();
        assert_eq!(groups.get(&info.id).unwrap().name, "Rodzinka");
        assert_eq!(groups.all_members(&me), vec![ola]);
        assert!(groups.knows(&ola));
        fs::remove_dir_all(dir).unwrap();
    }
}

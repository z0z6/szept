//! Archives: one encrypted file holding what is needed to carry on after a
//! reinstall or on a new phone.
//!
//! Layout of version 2: the magic bytes ending in the version, the Argon2id
//! settings (memory in KiB, passes, lanes), a random 32-byte salt and a
//! random 24-byte nonce, then the archive itself (postcard), padded to a
//! multiple of 64 KiB and sealed with XChaCha20-Poly1305. The key comes from
//! the password through Argon2id with 128 MiB of memory and three passes, so
//! every guess costs an attacker that much; the whole header is
//! authenticated, so its settings cannot be weakened unnoticed. Keys and the
//! opened archive are wiped from memory after use.
//!
//! A wrong password and a damaged file look the same: the seal does not open.
//! Version 1 archives (19 MiB, two passes, no padding) can still be read.

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use komunikator_core::{
    Contacts, EndpointId, GroupInfo, Groups, Identity, contacts, files, groups, identity,
    protocol::{MAX_AVATAR_LEN, MAX_MEDIA_SIZE},
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    Media, Shared, avatar_path, my_name_path, names_path,
    passwords::{MIN_PASSWORD_BITS, MIN_PASSWORD_LEN, estimate_bits},
    write_atomic,
};

/// The magic bytes without the format version, which follows them.
const MAGIC: &[u8; 15] = b"SZEPT-ARCHIWUM\x00";
const FORMAT_V1: u8 = 1;
const FORMAT_V2: u8 = 2;
const VERSION: u32 = 2;

/// Argon2id for new archives: 128 MiB, three passes, one lane.
const MEMORY_KIB: u32 = 128 * 1024;
const PASSES: u32 = 3;
const LANES: u32 = 1;

/// Settings an archive may ask for when opened: enough for any future
/// strengthening, not so much that a crafted file can exhaust the device.
const MAX_MEMORY_KIB: u32 = 1024 * 1024;
const MAX_PASSES: u32 = 16;
const MAX_LANES: u32 = 8;

/// The opened archive is padded to a multiple of this, hiding its exact size.
const PADDING: usize = 64 * 1024;

/// At most this many preferences travel in an archive.
const MAX_PREFERENCES: usize = 64;

/// One reaction: who (an identifier, or empty for us) and which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct ArchivedReaction {
    pub who: String,
    pub reaction: String,
}

/// One line of a conversation, as the app keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct ArchivedMessage {
    pub id: String,
    /// Who wrote it: an identifier, or empty for us.
    pub author: String,
    pub at_ms: u64,
    /// "text", "file", "sticker" or "system".
    pub kind: String,
    /// The text, the sticker code or the system note.
    pub body: String,
    pub file_name: Option<String>,
    pub file_size: u64,
    pub file_hash: Option<String>,
    pub media: Option<Media>,
    /// Where the file is on this device. On export, voice messages and GIFs
    /// found there go into the archive; on import, where they were put.
    pub file_path: Option<String>,
    /// "queued", "sent", "delivered" or "received".
    pub status: String,
    pub reactions: Vec<ArchivedReaction>,
}

/// One conversation: `key` is the contact's identifier or the group's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct ArchivedConversation {
    pub key: String,
    pub messages: Vec<ArchivedMessage>,
}

/// What an import brought back.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportedArchive {
    pub conversations: Vec<ArchivedConversation>,
    /// The identifier from before is back.
    pub identity_restored: bool,
    /// The archive held an identity key, used or not.
    pub had_identity: bool,
    pub contacts_added: u32,
    pub groups_added: u32,
    /// When the archive was written, in milliseconds since 1970.
    pub created_ms: u64,
    /// The person's preferences and look (theme, brightness, text size…),
    /// for the app to apply. Empty for archives from older versions.
    pub preferences: HashMap<String, String>,
}

/// The content of a version 2 archive.
#[derive(Serialize, Deserialize)]
struct Archive {
    version: u32,
    created_ms: u64,
    identity: Option<[u8; 32]>,
    my_name: String,
    my_avatar: Vec<u8>,
    contacts: Vec<(String, [u8; 32])>,
    groups: Vec<GroupInfo>,
    names: Vec<(String, String)>,
    avatars: Vec<(String, Vec<u8>)>,
    conversations: Vec<ArchivedConversation>,
    /// Voice messages and GIFs: message id, file name, content.
    blobs: Vec<(String, String, Vec<u8>)>,
    preferences: Vec<(String, String)>,
}

/// The content of a version 1 archive.
#[derive(Serialize, Deserialize)]
struct ArchiveV1 {
    version: u32,
    created_ms: u64,
    identity: Option<[u8; 32]>,
    my_name: String,
    my_avatar: Vec<u8>,
    contacts: Vec<(String, [u8; 32])>,
    groups: Vec<GroupInfo>,
    names: Vec<(String, String)>,
    avatars: Vec<(String, Vec<u8>)>,
    conversations: Vec<ArchivedConversation>,
    /// Voice messages and GIFs: message id, file name, content.
    blobs: Vec<(String, String, Vec<u8>)>,
}

impl From<ArchiveV1> for Archive {
    fn from(old: ArchiveV1) -> Self {
        Self {
            version: old.version,
            created_ms: old.created_ms,
            identity: old.identity,
            my_name: old.my_name,
            my_avatar: old.my_avatar,
            contacts: old.contacts,
            groups: old.groups,
            names: old.names,
            avatars: old.avatars,
            conversations: old.conversations,
            blobs: old.blobs,
            preferences: Vec::new(),
        }
    }
}

pub(crate) fn export(
    shared: &Shared,
    identity: Option<[u8; 32]>,
    path: &Path,
    password: &str,
    mut conversations: Vec<ArchivedConversation>,
    preferences: HashMap<String, String>,
) -> Result<()> {
    ensure!(
        password.chars().count() >= MIN_PASSWORD_LEN,
        "the password must have at least {MIN_PASSWORD_LEN} characters"
    );
    ensure!(
        estimate_bits(password) >= MIN_PASSWORD_BITS,
        "the password is too easy to guess"
    );
    let preferences = clean_preferences(preferences);
    let contacts: Vec<(String, [u8; 32])> = shared
        .contacts
        .lock()
        .unwrap()
        .iter()
        .map(|(alias, id)| (alias.to_owned(), *id.as_bytes()))
        .collect();
    let groups: Vec<GroupInfo> = shared.groups.lock().unwrap().iter().cloned().collect();
    let names: BTreeMap<String, String> = fs::read(&shared.names_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut avatars = Vec::new();
    if let Ok(entries) = fs::read_dir(&shared.avatars_dir) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = file.strip_suffix(".img") else {
                continue;
            };
            if let Ok(image) = fs::read(entry.path())
                && image.len() <= MAX_AVATAR_LEN
                && files::is_picture(&image)
            {
                avatars.push((id.to_owned(), image));
            }
        }
    }

    // Voice messages and GIFs travel inside; other files only by name.
    let mut blobs = Vec::new();
    for conversation in &mut conversations {
        for message in &mut conversation.messages {
            let path = message.file_path.take();
            if message.media.is_none() {
                continue;
            }
            let Some(path) = path else { continue };
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if metadata.len() > MAX_MEDIA_SIZE {
                continue;
            }
            if let Ok(bytes) = fs::read(&path) {
                let name = message
                    .file_name
                    .clone()
                    .filter(|name| files::is_safe_name(name))
                    .unwrap_or_else(|| "plik".into());
                blobs.push((message.id.clone(), name, bytes));
            }
        }
    }

    let archive = Archive {
        version: VERSION,
        created_ms: now_ms(),
        identity,
        my_name: shared.my_name.lock().unwrap().clone(),
        my_avatar: shared.avatar.lock().unwrap().clone(),
        contacts,
        groups,
        names: names.into_iter().collect(),
        avatars,
        conversations,
        blobs,
        preferences,
    };
    let plain = Zeroizing::new(postcard::to_stdvec(&archive).context("cannot pack the archive")?);
    drop(archive);
    let sealed = seal(&plain, password)?;
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    write_atomic(path, &sealed).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

pub(crate) fn import(
    data_dir: &Path,
    path: &Path,
    password: &str,
    restore_identity: bool,
) -> Result<ImportedArchive> {
    let sealed = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let (format, plain) = open(&sealed, password)?;
    let archive: Archive = if format == FORMAT_V1 {
        postcard::from_bytes::<ArchiveV1>(&plain)
            .context("the archive is damaged")?
            .into()
    } else {
        postcard::from_bytes(&plain).context("the archive is damaged")?
    };
    drop(plain);
    ensure!(
        archive.version <= VERSION,
        "this archive comes from a newer Szept; update the app first"
    );

    let identity_path = identity::default_path(data_dir);
    let identity_restored = match archive.identity {
        Some(secret) if restore_identity => {
            Identity::restore(&identity_path, secret)?;
            true
        }
        _ => false,
    };
    let me = if Identity::exists(&identity_path) {
        Some(Identity::load(&identity_path)?.id())
    } else {
        None
    };

    // Contacts: add who is missing; a taken name gets a number.
    let mut book = Contacts::load(&contacts::default_path(data_dir))?;
    let mut contacts_added = 0;
    for (alias, bytes) in &archive.contacts {
        let Ok(id) = EndpointId::from_bytes(bytes) else {
            continue;
        };
        if Some(id) == me || book.alias_of(&id).is_some() {
            continue;
        }
        let mut name = alias.clone();
        let mut number = 2;
        while book.get(&name).is_some() {
            name = format!("{alias}{number}");
            number += 1;
        }
        if book.add(&name, id).is_ok() {
            contacts_added += 1;
        }
    }

    let mut groups = Groups::load(&groups::default_path(data_dir))?;
    let mut groups_added = 0;
    for info in archive.groups {
        let mine = me.is_some_and(|me| info.has(&me));
        if mine && groups.accept(info)? {
            groups_added += 1;
        }
    }

    // Names and pictures of others fill gaps, never overwrite.
    let names_file = names_path(data_dir);
    let mut names: BTreeMap<String, String> = fs::read(&names_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    for (id, name) in archive.names {
        names.entry(id).or_insert(name);
    }
    write_atomic(&names_file, &serde_json::to_vec_pretty(&names)?)?;
    let avatars_dir = data_dir.join("avatars");
    fs::create_dir_all(&avatars_dir)?;
    for (id, image) in archive.avatars {
        let valid = id.parse::<EndpointId>().is_ok()
            && image.len() <= MAX_AVATAR_LEN
            && files::is_picture(&image);
        let target = avatars_dir.join(format!("{id}.img"));
        if valid && !target.exists() {
            write_atomic(&target, &image)?;
        }
    }

    // Our own name and picture come back with our identity, or into a gap.
    let name_file = my_name_path(data_dir);
    let current_name = fs::read_to_string(&name_file).unwrap_or_default();
    if !archive.my_name.is_empty() && (identity_restored || current_name.trim().is_empty()) {
        write_atomic(&name_file, archive.my_name.as_bytes())?;
    }
    let picture_file = avatar_path(data_dir);
    let valid_picture =
        archive.my_avatar.len() <= MAX_AVATAR_LEN && files::is_picture(&archive.my_avatar);
    if valid_picture && (identity_restored || !picture_file.exists()) {
        write_atomic(&picture_file, &archive.my_avatar)?;
    }

    // Voice messages and GIFs go back next to the others.
    let media_dir = data_dir.join("media").join("archiwum");
    let mut restored: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (id, name, bytes) in archive.blobs {
        if !files::is_safe_name(&name)
            || id.len() > 64
            || !id.bytes().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        fs::create_dir_all(&media_dir)?;
        let target = media_dir.join(format!("{id}-{name}"));
        write_atomic(&target, &bytes)?;
        restored.insert(id, target);
    }
    let mut conversations = archive.conversations;
    for conversation in &mut conversations {
        for message in &mut conversation.messages {
            message.file_path = restored
                .get(&message.id)
                .map(|path| path.to_string_lossy().into_owned());
        }
    }

    Ok(ImportedArchive {
        conversations,
        identity_restored,
        had_identity: archive.identity.is_some(),
        contacts_added,
        groups_added,
        created_ms: archive.created_ms,
        preferences: clean_preferences(archive.preferences.into_iter().collect())
            .into_iter()
            .collect(),
    })
}

/// Keeps a sensible number of short, printable preferences.
fn clean_preferences(preferences: HashMap<String, String>) -> Vec<(String, String)> {
    let mut kept: Vec<(String, String)> = preferences
        .into_iter()
        .filter(|(key, value)| {
            !key.is_empty()
                && key.len() <= 64
                && value.len() <= 256
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
                && !value.chars().any(char::is_control)
        })
        .collect();
    kept.sort();
    kept.truncate(MAX_PREFERENCES);
    kept
}

/// Argon2id settings, as stored in a version 2 header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kdf {
    memory_kib: u32,
    passes: u32,
    lanes: u32,
}

const KDF_V1: Kdf = Kdf {
    memory_kib: 19 * 1024,
    passes: 2,
    lanes: 1,
};

const KDF_V2: Kdf = Kdf {
    memory_kib: MEMORY_KIB,
    passes: PASSES,
    lanes: LANES,
};

fn key_from(password: &str, salt: &[u8], kdf: Kdf) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(kdf.memory_kib, kdf.passes, kdf.lanes, Some(32))
        .map_err(|err| anyhow!("bad key settings: {err}"))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|err| anyhow!("cannot derive the key: {err}"))?;
    Ok(key)
}

/// Pads `plain` with its length in front and zeros behind, to a multiple of
/// [`PADDING`].
fn pad(plain: &[u8]) -> Zeroizing<Vec<u8>> {
    let length = 8 + plain.len();
    let padded = length.div_ceil(PADDING) * PADDING;
    let mut out = Zeroizing::new(Vec::with_capacity(padded));
    out.extend_from_slice(&(plain.len() as u64).to_le_bytes());
    out.extend_from_slice(plain);
    out.resize(padded, 0);
    out
}

fn unpad(mut padded: Vec<u8>) -> Result<Zeroizing<Vec<u8>>> {
    let length = padded
        .get(..8)
        .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap_or_default()) as usize);
    let Some(length) = length.filter(|length| 8 + length <= padded.len()) else {
        padded.zeroize();
        bail!("the archive is damaged");
    };
    let plain = Zeroizing::new(padded[8..8 + length].to_vec());
    padded.zeroize();
    Ok(plain)
}

fn seal(plain: &[u8], password: &str) -> Result<Vec<u8>> {
    let salt: [u8; 32] = rand::random();
    let nonce: [u8; 24] = rand::random();
    let mut header = Vec::with_capacity(MAGIC.len() + 1 + 12 + 32 + 24);
    header.extend_from_slice(MAGIC);
    header.push(FORMAT_V2);
    for value in [KDF_V2.memory_kib, KDF_V2.passes, KDF_V2.lanes] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);

    let key = key_from(password, &salt, KDF_V2)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let padded = pad(plain);
    let sealed = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &padded,
                aad: &header,
            },
        )
        .map_err(|_| anyhow!("cannot encrypt the archive"))?;
    let mut out = header;
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Opens a sealed archive. Returns its format and the content.
fn open(sealed: &[u8], password: &str) -> Result<(u8, Zeroizing<Vec<u8>>)> {
    if sealed.len() < MAGIC.len() + 1 || sealed[..MAGIC.len()] != MAGIC[..] {
        bail!("this is not a Szept archive");
    }
    let format = sealed[MAGIC.len()];
    let rest = &sealed[MAGIC.len() + 1..];
    let (kdf, salt, nonce, body, header_len) = match format {
        FORMAT_V1 => {
            ensure!(rest.len() >= 40, "the archive is damaged");
            (
                KDF_V1,
                &rest[..16],
                &rest[16..40],
                &rest[40..],
                MAGIC.len() + 1,
            )
        }
        FORMAT_V2 => {
            ensure!(rest.len() >= 12 + 56, "the archive is damaged");
            let number =
                |at: usize| u32::from_le_bytes(rest[at..at + 4].try_into().unwrap_or_default());
            let kdf = Kdf {
                memory_kib: number(0),
                passes: number(4),
                lanes: number(8),
            };
            ensure!(
                kdf.memory_kib <= MAX_MEMORY_KIB
                    && kdf.passes <= MAX_PASSES
                    && kdf.lanes <= MAX_LANES,
                "the archive asks for unreasonable key settings"
            );
            (
                kdf,
                &rest[12..44],
                &rest[44..68],
                &rest[68..],
                MAGIC.len() + 1 + 68,
            )
        }
        _ => bail!("this archive comes from a newer Szept; update the app first"),
    };
    let key = key_from(password, salt, kdf)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let opened = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: body,
                aad: &sealed[..header_len],
            },
        )
        .map_err(|_| anyhow!("wrong password, or the archive is damaged"))?;
    let plain = if format == FORMAT_V1 {
        Zeroizing::new(opened)
    } else {
        unpad(opened)?
    };
    Ok((format, plain))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_data_opens_only_with_the_password() {
        let sealed = seal(b"rozmowy", "dobre haslo").unwrap();
        // The size of the content is hidden.
        assert!(sealed.len() > PADDING);
        let (format, plain) = open(&sealed, "dobre haslo").unwrap();
        assert_eq!((format, plain.as_slice()), (FORMAT_V2, &b"rozmowy"[..]));
        assert!(open(&sealed, "zle haslo").is_err());
        let mut broken = sealed.clone();
        *broken.last_mut().unwrap() ^= 1;
        assert!(open(&broken, "dobre haslo").is_err());
        assert!(open(b"not an archive at all, really not", "dobre haslo").is_err());
    }

    #[test]
    fn weakened_settings_in_the_header_are_caught() {
        let mut sealed = seal(b"rozmowy", "dobre haslo").unwrap();
        // Lower the memory from 128 MiB to 8 MiB: the seal no longer opens.
        sealed[MAGIC.len() + 1..MAGIC.len() + 5].copy_from_slice(&(8 * 1024u32).to_le_bytes());
        assert!(open(&sealed, "dobre haslo").is_err());
        // Absurd settings are refused before any work is done.
        sealed[MAGIC.len() + 1..MAGIC.len() + 5].copy_from_slice(&u32::MAX.to_le_bytes());
        let err = open(&sealed, "dobre haslo").unwrap_err().to_string();
        assert!(err.contains("unreasonable"), "{err}");
    }

    #[test]
    fn version_one_archives_still_open() {
        // Sealed the old way: 19 MiB, two passes, a 16-byte salt, no padding.
        let salt = [7u8; 16];
        let nonce = [9u8; 24];
        let mut header = MAGIC.to_vec();
        header.push(FORMAT_V1);
        let key = key_from("stare haslo", &salt, KDF_V1).unwrap();
        let cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
        let body = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: b"stare rozmowy",
                    aad: &header,
                },
            )
            .unwrap();
        let mut sealed = header;
        sealed.extend_from_slice(&salt);
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&body);
        let (format, plain) = open(&sealed, "stare haslo").unwrap();
        assert_eq!(
            (format, plain.as_slice()),
            (FORMAT_V1, &b"stare rozmowy"[..])
        );
    }
}

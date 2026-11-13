//! The interface of the messenger for graphical applications.
//!
//! UniFFI turns everything exported here into Kotlin. All calls are blocking
//! and quick, except [`App::start`], [`App::stop`], file offers and archives,
//! which may take a moment: call those off the main thread.

mod archive;
mod engine;
mod passwords;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    future::Future,
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail, ensure};
use komunikator_core::{
    Contacts, Content, EndpointAddr, EndpointId, FileOffer, GroupId, GroupInfo, Groups, Identity,
    Member, MessageId, Network, Node, Outgoing, Post, Signal, contacts, files, groups, identity,
    protocol::{MAX_AVATAR_LEN, MAX_GROUP_MEMBERS, MAX_MEDIA_SIZE, is_code, is_name},
};
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

pub use crate::archive::{
    ArchivedConversation, ArchivedMessage, ArchivedReaction, ImportedArchive,
};
use crate::engine::{Command, Item};
pub use crate::passwords::{
    PasswordStrength, min_password_len, password_strength, suggest_passphrase,
};

uniffi::setup_scaffolding!();

/// The only error type crossing the language boundary: a message for people.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum AppError {
    #[error("{0}")]
    Failed(String),
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::Failed(format!("{err:#}"))
    }
}

type Result<T> = std::result::Result<T, AppError>;

/// One entry of the address book.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Contact {
    pub alias: String,
    pub id: String,
}

/// What a file offer is for, when it is more than a plain file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum Media {
    /// A recorded voice message of `duration_ms` milliseconds.
    Voice { duration_ms: u32 },
    /// An animated picture or a sticker, usually inserted from the keyboard.
    Gif,
}

impl From<komunikator_core::Media> for Media {
    fn from(media: komunikator_core::Media) -> Self {
        match media {
            komunikator_core::Media::Voice { duration_ms } => Self::Voice { duration_ms },
            komunikator_core::Media::Gif => Self::Gif,
        }
    }
}

impl From<Media> for komunikator_core::Media {
    fn from(media: Media) -> Self {
        match media {
            Media::Voice { duration_ms } => Self::Voice { duration_ms },
            Media::Gif => Self::Gif,
        }
    }
}

/// Something that happened on the network. `peer` is the contact's
/// identifier. `group` is set for everything that belongs to a group chat.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum ChatEvent {
    Connected {
        peer: String,
    },
    Disconnected {
        peer: String,
    },
    Message {
        peer: String,
        id: String,
        sent_at_ms: u64,
        body: String,
        group: Option<String>,
    },
    /// The device of `peer` confirmed the receipt of our message `id`. In a
    /// group every member confirms separately.
    Delivered {
        peer: String,
        id: String,
        group: Option<String>,
    },
    /// Our message `id` left this device for `peer`. [`ChatEvent::Delivered`]
    /// follows once that device confirms it.
    Sent {
        peer: String,
        id: String,
        group: Option<String>,
    },
    /// A file is offered. Fetch it with [`App::download`].
    /// `hash` is the BLAKE3 hash in hex, for comparing by eye.
    ///
    /// With `media` set it is a voice message or a GIF: the app starts
    /// fetching it on its own into [`App::media_dir`], and reports that
    /// like any other download.
    FileOffered {
        peer: String,
        id: String,
        sent_at_ms: u64,
        name: String,
        size: u64,
        hash: String,
        media: Option<Media>,
        group: Option<String>,
    },
    /// A download moved forward: `done` of `total` bytes.
    FileProgress {
        peer: String,
        id: String,
        done: u64,
        total: u64,
    },
    /// A download finished, was verified and saved at `path`.
    FileSaved {
        peer: String,
        id: String,
        path: String,
    },
    /// A download stopped. Calling [`App::download`] again resumes it.
    FileFailed {
        peer: String,
        id: String,
        reason: String,
    },
    /// The contact set, changed or removed their profile picture. Read it
    /// with [`App::avatar`].
    AvatarChanged {
        peer: String,
    },
    /// The contact changed their display name. Read it with [`App::display_name`].
    NameChanged {
        peer: String,
    },
    /// An animated sticker, by its code.
    Sticker {
        peer: String,
        id: String,
        sent_at_ms: u64,
        code: String,
        group: Option<String>,
    },
    /// `peer` reacted to the message `target` with `reaction`, or took the
    /// reaction back when it is empty. Each person has one reaction per message.
    Reaction {
        peer: String,
        target: String,
        reaction: String,
        group: Option<String>,
    },
    /// A group was created, renamed or its members changed. Read it with [`App::groups`].
    GroupChanged {
        group: String,
    },
    /// We are no longer in a group: `reason` is "removed" when the owner
    /// took us out, "closed" when the owner closed it.
    GroupClosed {
        group: String,
        reason: String,
    },
}

/// A file just offered to a contact or a group.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SentFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub hash: String,
}

/// One person in a group. `name` is what the owner calls them: show the
/// contact's alias or their own display name instead when there is one.
#[derive(Debug, Clone, uniffi::Record)]
pub struct GroupMember {
    pub id: String,
    pub name: String,
}

/// A group chat.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub owner: String,
    /// `true` when we own it: only the owner renames it and changes members.
    pub mine: bool,
    /// Everyone, the owner and us included.
    pub members: Vec<GroupMember>,
}

impl Group {
    fn from_info(info: &GroupInfo, me: &EndpointId) -> Self {
        Self {
            id: info.id.to_string(),
            name: info.name.clone(),
            owner: info.owner().map(|id| id.to_string()).unwrap_or_default(),
            mine: info.owner == *me.as_bytes(),
            members: info
                .members
                .iter()
                .filter_map(|member| {
                    Some(GroupMember {
                        id: member.endpoint()?.to_string(),
                        name: member.name.clone(),
                    })
                })
                .collect(),
        }
    }
}

/// A QR code: `size` × `size` modules, row by row, `true` meaning dark.
#[derive(Debug, Clone, uniffi::Record)]
pub struct QrCode {
    pub size: u32,
    pub modules: Vec<bool>,
}

/// The text put into a QR code to share an identifier: `szept:<identifier>`.
///
/// The prefix lets a scanner tell a Szept code from any other, and every
/// place that accepts an identifier also accepts it with the prefix.
#[uniffi::export]
pub fn share_link(id: String) -> String {
    format!("{ID_PREFIX}{}", id.trim())
}

/// Encodes `text` as a QR code with medium error correction.
#[uniffi::export]
pub fn qr_code(text: String) -> Result<QrCode> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
        .map_err(|err| anyhow!("cannot make a QR code: {err}"))?;
    let size = code.width();
    let modules = code
        .to_colors()
        .into_iter()
        .map(|color| color == qrcode::Color::Dark)
        .collect();
    Ok(QrCode {
        size: u32::try_from(size).context("QR code too large")?,
        modules,
    })
}

/// Turns a scanned or pasted identifier into its canonical form.
///
/// Accepts the bare identifier and the `szept:` link, ignoring whitespace.
#[uniffi::export]
pub fn normalize_id(text: String) -> Result<String> {
    Ok(parse_id(&text)?.to_string())
}

/// What kind of file a name suggests: "program", "archive", "image",
/// "audio", "video", "document" or "other". Programs are never opened by
/// the app itself, only shown in their folder.
#[uniffi::export]
pub fn file_kind(name: String) -> String {
    files::kind(&name).to_owned()
}

/// The largest voice message or GIF, in bytes.
#[uniffi::export]
pub fn max_media_size() -> u64 {
    MAX_MEDIA_SIZE
}

/// The largest profile picture, in bytes. Scale pictures down to fit.
#[uniffi::export]
pub fn max_avatar_len() -> u64 {
    MAX_AVATAR_LEN as u64
}

/// Receives events. Called on a background thread.
#[uniffi::export(with_foreign)]
pub trait EventListener: Send + Sync {
    fn on_event(&self, event: ChatEvent);
}

/// The messenger. Create one per process.
#[derive(uniffi::Object)]
pub struct App {
    runtime: Runtime,
    identity: Identity,
    network: Network,
    running: Mutex<Option<Running>>,
    shared: Arc<Shared>,
}

/// What the commands and the background engine both use.
pub(crate) struct Shared {
    pub(crate) me: EndpointId,
    pub(crate) contacts: Mutex<Contacts>,
    pub(crate) groups: Mutex<Groups>,
    /// Display names people chose for themselves, by identifier.
    names: Mutex<BTreeMap<String, String>>,
    pub(crate) names_path: PathBuf,
    /// Our own display name, empty when there is none.
    pub(crate) my_name: Mutex<String>,
    my_name_path: PathBuf,
    /// Everyone the node accepts: contacts and the members of our groups.
    allowed: Mutex<HashSet<EndpointId>>,
    /// Files offered to us, by message id.
    pub(crate) offers: Mutex<HashMap<String, (EndpointId, MessageId, FileOffer)>>,
    /// Message ids of the downloads in progress.
    pub(crate) downloading: Mutex<HashSet<String>>,
    /// Where voice messages and GIFs land.
    pub(crate) media_dir: PathBuf,
    /// Other people's profile pictures, one file per identifier.
    pub(crate) avatars_dir: PathBuf,
    /// Our own profile picture on disk.
    avatar_path: PathBuf,
    /// Our own profile picture, empty when there is none.
    pub(crate) avatar: Mutex<Vec<u8>>,
}

impl Shared {
    pub(crate) fn avatar_file(&self, peer: &EndpointId) -> PathBuf {
        self.avatars_dir.join(format!("{peer}.img"))
    }

    /// Stores what someone sent as their picture. Returns `true` if it
    /// differs from what was stored before.
    pub(crate) fn store_avatar(&self, peer: &EndpointId, image: &[u8]) -> bool {
        let path = self.avatar_file(peer);
        let old = fs::read(&path).unwrap_or_default();
        if old == image {
            return false;
        }
        let saved = if image.is_empty() {
            fs::remove_file(&path)
        } else {
            fs::create_dir_all(&self.avatars_dir).and_then(|()| write_atomic(&path, image))
        };
        saved.is_ok()
    }

    /// Stores someone's display name. Returns `true` on a change.
    pub(crate) fn store_name(&self, peer: &EndpointId, name: &str) -> bool {
        let mut names = self.names.lock().unwrap();
        let key = peer.to_string();
        if names.get(&key).map(String::as_str) == Some(name) {
            return false;
        }
        names.insert(key, name.to_owned());
        let _ = save_json(&self.names_path, &*names);
        true
    }

    pub(crate) fn forget(&self, peer: &EndpointId) {
        let _ = fs::remove_file(self.avatar_file(peer));
        let mut names = self.names.lock().unwrap();
        if names.remove(&peer.to_string()).is_some() {
            let _ = save_json(&self.names_path, &*names);
        }
    }

    pub(crate) fn is_contact(&self, peer: &EndpointId) -> bool {
        self.contacts.lock().unwrap().alias_of(peer).is_some()
    }

    /// May `peer` post into `group` (or to us directly, for `None`)?
    pub(crate) fn may_post(&self, peer: &EndpointId, group: Option<&GroupId>) -> bool {
        match group {
            None => self.is_contact(peer),
            Some(group) => self
                .groups
                .lock()
                .unwrap()
                .get(group)
                .is_some_and(|info| info.has(peer)),
        }
    }

    /// Lets in exactly the contacts and the members of our groups.
    pub(crate) fn sync_allowed(&self, node: &Node) {
        let mut wanted: HashSet<EndpointId> = self
            .contacts
            .lock()
            .unwrap()
            .iter()
            .map(|(_, id)| id)
            .collect();
        wanted.extend(self.groups.lock().unwrap().all_members(&self.me));
        let mut allowed = self.allowed.lock().unwrap();
        for gone in allowed.difference(&wanted) {
            node.disallow(gone);
        }
        for new in wanted.difference(&allowed) {
            node.allow(*new);
        }
        *allowed = wanted;
    }

    /// The display name to send, if we have one.
    pub(crate) fn name_signal(&self) -> Option<Signal> {
        let name = self.my_name.lock().unwrap().clone();
        is_name(&name).then_some(Signal::Name(name))
    }
}

fn check_avatar(image: &[u8]) -> anyhow::Result<()> {
    ensure!(
        image.len() <= MAX_AVATAR_LEN,
        "the picture is too large ({} bytes, the limit is {MAX_AVATAR_LEN})",
        image.len()
    );
    ensure!(
        files::is_picture(image),
        "the picture must be a JPEG, PNG, WebP or GIF"
    );
    Ok(())
}

/// Writes `bytes` next to `path` first, then moves them into place, so a
/// crash never leaves half a file behind.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let part = path.with_extension("part");
    fs::write(&part, bytes)?;
    fs::rename(&part, path)
}

fn save_json(path: &Path, value: &impl serde::Serialize) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomic(path, &serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub(crate) fn names_path(data_dir: &Path) -> PathBuf {
    data_dir.join("names.json")
}

pub(crate) fn my_name_path(data_dir: &Path) -> PathBuf {
    data_dir.join("name.txt")
}

pub(crate) fn avatar_path(data_dir: &Path) -> PathBuf {
    data_dir.join("avatar.img")
}

/// Where a message goes: one contact, or everyone in a group.
enum Target {
    Peer(EndpointId),
    Group(GroupInfo),
}

struct Running {
    node: Node,
    commands: mpsc::UnboundedSender<Command>,
    engine: JoinHandle<()>,
    listener: Arc<dyn EventListener>,
}

#[uniffi::export]
impl App {
    /// Opens the data directory, creating a new identity on first use.
    ///
    /// With `local_only` the app contacts nothing but the other device, and
    /// contacts must be dialled with [`App::connect_direct`].
    #[uniffi::constructor]
    pub fn new(data_dir: String, local_only: bool) -> Result<Arc<Self>> {
        let data_dir = PathBuf::from(data_dir);
        let identity_path = identity::default_path(&data_dir);
        let identity = if Identity::exists(&identity_path) {
            Identity::load(&identity_path)?
        } else {
            Identity::create(&identity_path)?
        };
        let contacts = Contacts::load(&contacts::default_path(&data_dir))?;
        let groups = Groups::load(&groups::default_path(&data_dir))?;
        let names: BTreeMap<String, String> = fs::read(names_path(&data_dir))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let my_name = fs::read_to_string(my_name_path(&data_dir))
            .map(|name| name.trim().to_owned())
            .unwrap_or_default();
        let avatar = fs::read(avatar_path(&data_dir))
            .ok()
            .filter(|image| image.len() <= MAX_AVATAR_LEN && files::is_picture(image))
            .unwrap_or_default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("szept")
            .enable_all()
            .build()
            .context("cannot start the async runtime")?;
        Ok(Arc::new(Self {
            runtime,
            network: if local_only {
                Network::LocalOnly
            } else {
                Network::Public
            },
            running: Mutex::new(None),
            shared: Arc::new(Shared {
                me: identity.id(),
                contacts: Mutex::new(contacts),
                groups: Mutex::new(groups),
                names: Mutex::new(names),
                names_path: names_path(&data_dir),
                my_name: Mutex::new(my_name),
                my_name_path: my_name_path(&data_dir),
                allowed: Mutex::default(),
                offers: Mutex::default(),
                downloading: Mutex::default(),
                media_dir: data_dir.join("media"),
                avatars_dir: data_dir.join("avatars"),
                avatar_path: avatar_path(&data_dir),
                avatar: Mutex::new(avatar),
            }),
            identity,
        }))
    }

    /// The identifier to hand to other people.
    pub fn my_id(&self) -> String {
        self.identity.id().to_string()
    }

    /// The address book, sorted by alias.
    pub fn contacts(&self) -> Vec<Contact> {
        let contacts = self.shared.contacts.lock().unwrap();
        contacts
            .iter()
            .map(|(alias, id)| Contact {
                alias: alias.to_owned(),
                id: id.to_string(),
            })
            .collect()
    }

    /// Saves a contact and starts accepting connections from it.
    pub fn add_contact(&self, alias: String, id: String) -> Result<()> {
        let id = parse_id(&id)?;
        if id == self.identity.id() {
            return Err(anyhow!("this is your own identifier").into());
        }
        self.shared.contacts.lock().unwrap().add(&alias, id)?;
        self.sync_allowed();
        Ok(())
    }

    /// Deletes a contact. The connection closes unless they share a group with us.
    pub fn remove_contact(&self, alias: String) -> Result<()> {
        let id = {
            let mut contacts = self.shared.contacts.lock().unwrap();
            let id = contacts
                .get(&alias)
                .with_context(|| format!("unknown contact `{alias}`"))?;
            contacts.remove(&alias)?;
            id
        };
        if !self.shared.groups.lock().unwrap().knows(&id) {
            self.shared.forget(&id);
        }
        self.sync_allowed();
        let _ = self.command(Command::ContactRemoved(id));
        Ok(())
    }

    /// Gives the contact `alias` the new identifier `id`, after that person
    /// reinstalled Szept and got a new key. The old key stops being
    /// accepted; its queued messages are dropped. Groups we own follow the
    /// change. Returns the old identifier, so the conversation can be moved
    /// over.
    pub fn update_contact_key(&self, alias: String, id: String) -> Result<String> {
        let new = parse_id(&id)?;
        if new == self.identity.id() {
            return Err(anyhow!("this is your own identifier").into());
        }
        let old = self
            .shared
            .contacts
            .lock()
            .unwrap()
            .replace_id(&alias, new)?;
        self.shared.forget(&old);
        let _ = self.command(Command::ContactRemoved(old));
        // Our groups get the new key in place of the old one.
        let owned: Vec<GroupInfo> = {
            let groups = self.shared.groups.lock().unwrap();
            groups
                .iter()
                .filter(|info| info.owner == *self.identity.id().as_bytes() && info.has(&old))
                .cloned()
                .collect()
        };
        for mut info in owned {
            for member in &mut info.members {
                if member.id == *old.as_bytes() {
                    member.id = *new.as_bytes();
                }
            }
            info.version += 1;
            let _ = self.publish_group(info, vec![]);
        }
        self.sync_allowed();
        Ok(old.to_string())
    }

    /// Goes online and reports everything that happens to `listener`.
    pub fn start(&self, listener: Arc<dyn EventListener>) -> Result<()> {
        let mut running = self.running.lock().unwrap();
        if running.is_some() {
            return Ok(());
        }
        let (node, events) =
            self.runtime
                .block_on(Node::spawn(&self.identity, [], self.network))?;
        self.shared.allowed.lock().unwrap().clear();
        self.shared.sync_allowed(&node);
        let (commands, commands_rx) = mpsc::unbounded_channel();
        let engine = self.runtime.spawn(engine::run(
            node.clone(),
            events,
            commands_rx,
            listener.clone(),
            self.shared.clone(),
            // Without a lookup service there is nobody to ask for addresses.
            self.network == Network::Public,
        ));
        *running = Some(Running {
            node,
            commands,
            engine,
            listener,
        });
        Ok(())
    }

    /// Goes offline. Messages still waiting in the queue are dropped.
    pub fn stop(&self) {
        if let Some(running) = self.running.lock().unwrap().take() {
            running.engine.abort();
            self.runtime.block_on(running.node.shutdown());
        }
    }

    /// Queues a text message for a contact or a group (`to` is either
    /// identifier) and returns its identifier. It leaves as soon as each
    /// recipient is reachable.
    pub fn send(&self, to: String, body: String) -> Result<String> {
        let message = match self.target(&to)? {
            Target::Peer(_) => Outgoing::new(body)?,
            Target::Group(ref info) => Outgoing::post(Some(info.id), Post::Text(body))?,
        };
        self.queue_message(&to, message)
    }

    /// Sends one of the animated stickers by its code, to a contact or a group.
    pub fn send_sticker(&self, to: String, code: String) -> Result<String> {
        let group = match self.target(&to)? {
            Target::Peer(_) => None,
            Target::Group(info) => Some(info.id),
        };
        let message = Outgoing::post(group, Post::Sticker(code))?;
        self.queue_message(&to, message)
    }

    /// Reacts to the message `target` in the conversation `to` (a contact or
    /// a group) with `reaction`, or takes our reaction back when it is empty.
    pub fn react(&self, to: String, target: String, reaction: String) -> Result<()> {
        let target: MessageId = target.parse()?;
        if !reaction.is_empty() && !is_code(&reaction) {
            return Err(anyhow!("bad reaction").into());
        }
        let (peers, group) = match self.target(&to)? {
            Target::Peer(peer) => (vec![peer], None),
            Target::Group(info) => (info.others(&self.identity.id()), Some(info.id)),
        };
        let signal = Signal::React {
            target,
            group,
            reaction,
        };
        self.command(Command::Queue {
            peers,
            item: Item::Signal(signal),
            group: None,
        })
    }

    /// Offers the file at `path` to a contact or a group and returns what
    /// was offered.
    ///
    /// The file stays where it is: recipients fetch it straight from this
    /// device, so it must not move or change until then, and this device must
    /// be online. Reads the whole file to hash it: call off the main thread,
    /// and not from inside an async runtime.
    pub fn send_file(&self, to: String, path: String) -> Result<SentFile> {
        self.offer_file(&to, &path, None)
    }

    /// Sends a voice message or a GIF stored at `path`. Works like
    /// [`App::send_file`], except that recipients' apps fetch it at once.
    ///
    /// Voice messages must be `.m4a`, `.aac`, `.ogg`, `.oga`, `.opus`,
    /// `.webm`, `.mp3` or `.wav`, GIFs `.gif`, `.webp`, `.png`, `.jpg` or
    /// `.jpeg`, both at most [`max_media_size`] bytes.
    pub fn send_media(&self, to: String, path: String, media: Media) -> Result<SentFile> {
        self.offer_file(&to, &path, Some(media.into()))
    }

    /// Our display name, empty when we have none.
    pub fn my_name(&self) -> String {
        self.shared.my_name.lock().unwrap().clone()
    }

    /// Sets our display name, shown to contacts and to the other members of
    /// our groups. An empty name removes it locally; contacts keep the last one.
    pub fn set_name(&self, name: String) -> Result<()> {
        let name = name.trim().to_owned();
        if !name.is_empty() && !is_name(&name) {
            return Err(anyhow!("the name is too long").into());
        }
        if let Some(dir) = self.shared.my_name_path.parent() {
            fs::create_dir_all(dir).context("cannot save the name")?;
        }
        write_atomic(&self.shared.my_name_path, name.as_bytes()).context("cannot save the name")?;
        *self.shared.my_name.lock().unwrap() = name;
        let _ = self.command(Command::ProfileChanged);
        Ok(())
    }

    /// The display name `peer` chose for themselves, if they sent one.
    pub fn display_name(&self, peer: String) -> Option<String> {
        let peer = parse_id(&peer).ok()?;
        self.shared
            .names
            .lock()
            .unwrap()
            .get(&peer.to_string())
            .cloned()
    }

    /// Our profile picture, if there is one.
    pub fn my_avatar(&self) -> Option<Vec<u8>> {
        let avatar = self.shared.avatar.lock().unwrap();
        (!avatar.is_empty()).then(|| avatar.clone())
    }

    /// Sets our profile picture, or removes it when `image` is empty, and
    /// sends it to everyone: right away to those connected, on the next
    /// connection to the others.
    ///
    /// The picture must be a JPEG, PNG, WebP or GIF of at most
    /// [`max_avatar_len`] bytes: scale it down first. A square of 256 × 256
    /// pixels in JPEG fits easily.
    pub fn set_avatar(&self, image: Vec<u8>) -> Result<()> {
        if image.is_empty() {
            match fs::remove_file(&self.shared.avatar_path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(anyhow!("cannot remove the picture: {err}").into()),
            }
        } else {
            check_avatar(&image)?;
            write_atomic(&self.shared.avatar_path, &image).context("cannot save the picture")?;
        }
        *self.shared.avatar.lock().unwrap() = image;
        // Offline is fine: the picture goes out with the next connection.
        let _ = self.command(Command::ProfileChanged);
        Ok(())
    }

    /// The profile picture `peer` sent us, if any. Arrives with
    /// [`ChatEvent::AvatarChanged`] and stays after a restart.
    pub fn avatar(&self, peer: String) -> Option<Vec<u8>> {
        let peer = parse_id(&peer).ok()?;
        fs::read(self.shared.avatar_file(&peer))
            .ok()
            .filter(|image| image.len() <= MAX_AVATAR_LEN && files::is_picture(image))
    }

    /// Where voice messages and GIFs are saved.
    pub fn media_dir(&self) -> String {
        self.shared.media_dir.to_string_lossy().into_owned()
    }

    /// The groups we are in, sorted by name.
    pub fn groups(&self) -> Vec<Group> {
        let me = self.identity.id();
        let mut groups: Vec<Group> = self
            .shared
            .groups
            .lock()
            .unwrap()
            .iter()
            .map(|info| Group::from_info(info, &me))
            .collect();
        groups.sort_by_key(|group| group.name.to_lowercase());
        groups
    }

    /// Creates a group of us and the contacts `members`, and invites them.
    /// Members who are not each other's contacts can still talk in the group.
    pub fn create_group(&self, name: String, members: Vec<String>) -> Result<Group> {
        let me = self.identity.id();
        let info = GroupInfo {
            id: GroupId::random(),
            owner: *me.as_bytes(),
            version: 1,
            name: name.trim().to_owned(),
            members: self.roster(&members)?,
        };
        self.publish_group(info.clone(), vec![])?;
        Ok(Group::from_info(&info, &me))
    }

    /// Renames a group we own and sets its members (contacts' identifiers,
    /// without us). People taken out are told so.
    pub fn update_group(&self, group: String, name: String, members: Vec<String>) -> Result<Group> {
        let me = self.identity.id();
        let old = self.owned_group(&group)?;
        let mut roster = self.roster(&members)?;
        // People not in our contacts keep the name the group knew them by.
        for member in &mut roster {
            if let Some(known) = old.members.iter().find(|known| known.id == member.id)
                && member.name.is_empty()
            {
                member.name = known.name.clone();
            }
        }
        let info = GroupInfo {
            name: name.trim().to_owned(),
            members: roster,
            version: old.version + 1,
            ..old.clone()
        };
        let removed: Vec<EndpointId> = old
            .others(&me)
            .into_iter()
            .filter(|peer| !info.has(peer))
            .collect();
        self.publish_group(info.clone(), removed)?;
        Ok(Group::from_info(&info, &me))
    }

    /// Leaves a group. When we own it, this closes it for everyone.
    pub fn leave_group(&self, group: String) -> Result<()> {
        let id: GroupId = group.parse()?;
        let info = self
            .shared
            .groups
            .lock()
            .unwrap()
            .remove(&id)?
            .context("unknown group")?;
        self.sync_allowed();
        let _ = self.command(Command::Queue {
            peers: info.others(&self.identity.id()),
            item: Item::Signal(Signal::Leave(id)),
            group: None,
        });
        Ok(())
    }

    /// Downloads the file offered as `id` into the directory `dir`, in the
    /// background. Progress arrives as [`ChatEvent::FileProgress`], the end as
    /// [`ChatEvent::FileSaved`] or [`ChatEvent::FileFailed`]. Calling it again
    /// after a failure resumes where the transfer stopped.
    pub fn download(&self, id: String, dir: String) -> Result<()> {
        let (node, listener) = {
            let running = self.running.lock().unwrap();
            let Some(running) = &*running else {
                return Err(anyhow!("the app is offline").into());
            };
            (running.node.clone(), running.listener.clone())
        };
        if let Some(task) = download_task(&self.shared, node, listener, id, PathBuf::from(dir))? {
            self.runtime.spawn(task);
        }
        Ok(())
    }

    /// Stops offering a file sent earlier.
    pub fn unshare(&self, id: String) -> Result<()> {
        let id: MessageId = id.parse()?;
        self.node()?.unshare(&id);
        Ok(())
    }

    /// Writes an encrypted archive of everything needed to carry on after a
    /// reinstall: optionally our identity key, contacts, groups, names,
    /// pictures, the conversations the app hands over, with their voice
    /// messages and GIFs, and the app's `preferences` (theme, text size…).
    /// Other files are listed, not included.
    ///
    /// The password must pass [`password_strength`]. Deriving the key takes
    /// 128 MiB of memory and about a second.
    ///
    /// Without `include_identity` a restored app keeps its new identifier,
    /// and contacts must update our key. Reads files: call off the main thread.
    pub fn export_archive(
        &self,
        path: String,
        password: String,
        include_identity: bool,
        conversations: Vec<ArchivedConversation>,
        preferences: HashMap<String, String>,
    ) -> Result<()> {
        archive::export(
            &self.shared,
            include_identity.then(|| self.identity.secret_bytes()),
            Path::new(&path),
            &password,
            conversations,
            preferences,
        )?;
        Ok(())
    }

    /// Call when the device switches networks (Wi-Fi, mobile data, none):
    /// the app then finds its new addresses and reconnects at once instead
    /// of at the next periodic retry. Harmless when nothing changed.
    pub fn network_changed(&self) {
        let _ = self.command(Command::NetworkChanged);
    }

    /// Returns `true` if a connection with `peer` is up.
    pub fn is_online(&self, peer: String) -> bool {
        let Ok(peer) = parse_id(&peer) else {
            return false;
        };
        let running = self.running.lock().unwrap();
        running
            .as_ref()
            .is_some_and(|running| running.node.is_connected(&peer))
    }

    /// Dials a contact at a known `IP:PORT`, without any lookup service.
    pub fn connect_direct(&self, peer: String, address: String) -> Result<()> {
        let peer = parse_id(&peer)?;
        let address: SocketAddr = address
            .trim()
            .parse()
            .context("the address must look like 192.168.1.20:4433")?;
        self.command(Command::Dial(EndpointAddr::new(peer).with_ip_addr(address)))
    }

    /// The UDP port this device listens on, or 0 while offline.
    pub fn listening_port(&self) -> u16 {
        let running = self.running.lock().unwrap();
        running
            .as_ref()
            .and_then(|running| {
                let sockets = running.node.bound_sockets();
                sockets.into_iter().find(SocketAddr::is_ipv4)
            })
            .map_or(0, |socket| socket.port())
    }
}

/// Reads an archive written by [`App::export_archive`] into the data
/// directory, and returns its conversations for the app to show.
///
/// Call it while no [`App`] is running on `data_dir`, then create one anew.
/// Contacts, groups, names and pictures are merged with what is there. With
/// `restore_identity`, and when the archive holds one, the identity key is
/// replaced by the archived one: the identifier from before the reinstall
/// comes back, and the one created since is gone for good.
#[uniffi::export]
pub fn import_archive(
    data_dir: String,
    path: String,
    password: String,
    restore_identity: bool,
) -> Result<ImportedArchive> {
    Ok(archive::import(
        Path::new(&data_dir),
        Path::new(&path),
        &password,
        restore_identity,
    )?)
}

impl App {
    fn target(&self, to: &str) -> Result<Target> {
        if let Ok(group) = to.trim().parse::<GroupId>() {
            let info = self
                .shared
                .groups
                .lock()
                .unwrap()
                .get(&group)
                .cloned()
                .context("you are not in this group")?;
            return Ok(Target::Group(info));
        }
        Ok(Target::Peer(parse_id(to)?))
    }

    fn owned_group(&self, group: &str) -> Result<GroupInfo> {
        let Target::Group(info) = self.target(group)? else {
            return Err(anyhow!("unknown group").into());
        };
        if info.owner != *self.identity.id().as_bytes() {
            return Err(anyhow!("only the owner can change the group").into());
        }
        Ok(info)
    }

    /// Us plus the contacts `members`, named as we know them.
    fn roster(&self, members: &[String]) -> Result<Vec<Member>> {
        let me = self.identity.id();
        let my_name = self.my_name();
        let mut roster = vec![Member {
            id: *me.as_bytes(),
            name: my_name,
        }];
        let contacts = self.shared.contacts.lock().unwrap();
        let groups = self.shared.groups.lock().unwrap();
        for member in members {
            let id = parse_id(member)?;
            if id == me || roster.iter().any(|known| known.id == *id.as_bytes()) {
                continue;
            }
            // Contacts and people from our other groups may join.
            let name = match contacts.alias_of(&id) {
                Some(alias) => self
                    .shared
                    .names
                    .lock()
                    .unwrap()
                    .get(&id.to_string())
                    .cloned()
                    .unwrap_or_else(|| alias.to_owned()),
                None if groups.knows(&id) => String::new(),
                None => return Err(anyhow!("only contacts can be added to a group").into()),
            };
            roster.push(Member {
                id: *id.as_bytes(),
                name,
            });
        }
        if roster.len() > MAX_GROUP_MEMBERS {
            return Err(anyhow!("a group has at most {MAX_GROUP_MEMBERS} members").into());
        }
        if roster.len() < 2 {
            return Err(anyhow!("a group needs at least one other member").into());
        }
        Ok(roster)
    }

    /// Stores a group we own and sends it to its members and to `removed`.
    fn publish_group(&self, info: GroupInfo, removed: Vec<EndpointId>) -> Result<()> {
        if !is_name(&info.name) {
            return Err(anyhow!("the group needs a name").into());
        }
        self.shared.groups.lock().unwrap().accept(info.clone())?;
        self.sync_allowed();
        let mut peers = info.others(&self.identity.id());
        peers.extend(removed);
        let _ = self.command(Command::Queue {
            peers,
            item: Item::Signal(Signal::Group(info)),
            group: None,
        });
        Ok(())
    }

    fn queue_message(&self, to: &str, message: Outgoing) -> Result<String> {
        let id = message.id.to_string();
        let (peers, group) = match self.target(to)? {
            Target::Peer(peer) => (vec![peer], None),
            Target::Group(info) => (info.others(&self.identity.id()), Some(info.id)),
        };
        self.command(Command::Queue {
            peers,
            item: Item::Message(message),
            group,
        })?;
        Ok(id)
    }

    fn offer_file(
        &self,
        to: &str,
        path: &str,
        media: Option<komunikator_core::Media>,
    ) -> Result<SentFile> {
        let node = self.node()?;
        let message = match self.target(to)? {
            Target::Peer(peer) => match media {
                None => self.runtime.block_on(node.share(peer, Path::new(path)))?,
                Some(media) => {
                    self.runtime
                        .block_on(node.share_media(peer, Path::new(path), media))?
                }
            },
            Target::Group(info) => self.runtime.block_on(node.share_post(
                info.others(&self.identity.id()),
                Some(info.id),
                Path::new(path),
                media,
            ))?,
        };
        let sent = match &message.content {
            Content::File(offer)
            | Content::Media(offer, _)
            | Content::Post {
                post: Post::File { offer, .. },
                ..
            } => SentFile {
                id: message.id.to_string(),
                name: offer.name.clone(),
                size: offer.size,
                hash: files::hash_hex(&offer.hash),
            },
            _ => unreachable!("share makes a file offer"),
        };
        self.queue_message(to, message)?;
        Ok(sent)
    }

    fn sync_allowed(&self) {
        if let Ok(node) = self.node() {
            self.shared.sync_allowed(&node);
        }
    }

    fn node(&self) -> Result<Node> {
        let running = self.running.lock().unwrap();
        running
            .as_ref()
            .map(|running| running.node.clone())
            .ok_or_else(|| anyhow!("the app is offline").into())
    }

    fn command(&self, command: Command) -> Result<()> {
        let running = self.running.lock().unwrap();
        let Some(running) = &*running else {
            return Err(anyhow!("the app is offline").into());
        };
        running
            .commands
            .send(command)
            .map_err(|_| anyhow!("the app is shutting down"))?;
        Ok(())
    }
}

/// Prepares the download of the file offered as `id` into `dir`. Returns
/// `None` while that download is already running. Spawn the task on the
/// app's runtime.
pub(crate) fn download_task(
    shared: &Arc<Shared>,
    node: Node,
    listener: Arc<dyn EventListener>,
    id: String,
    dir: PathBuf,
) -> anyhow::Result<Option<impl Future<Output = ()> + Send + 'static>> {
    let (peer, message_id, offer) = shared
        .offers
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .context("this file offer is no longer known")?;
    if !shared.downloading.lock().unwrap().insert(id.clone()) {
        return Ok(None);
    }
    let shared = shared.clone();
    Ok(Some(async move {
        let peer_text = peer.to_string();
        let total = offer.size;
        let mut last = (Instant::now(), 0u64);
        let progress = |done: u64| {
            // At most about five reports a second, plus the last one.
            let due = last.0.elapsed() >= Duration::from_millis(200) || done == total;
            if due && (done != last.1 || done == 0) {
                last = (Instant::now(), done);
                listener.on_event(ChatEvent::FileProgress {
                    peer: peer_text.clone(),
                    id: id.clone(),
                    done,
                    total,
                });
            }
        };
        let result = node
            .download(peer, message_id, &offer, &dir, progress)
            .await;
        shared.downloading.lock().unwrap().remove(&id);
        listener.on_event(match result {
            Ok(path) => ChatEvent::FileSaved {
                peer: peer.to_string(),
                id,
                path: path.to_string_lossy().into_owned(),
            },
            Err(err) => ChatEvent::FileFailed {
                peer: peer.to_string(),
                id,
                reason: format!("{err:#}"),
            },
        });
    }))
}

/// Prefix of the identifier links put into QR codes.
const ID_PREFIX: &str = "szept:";

fn parse_id(text: &str) -> anyhow::Result<EndpointId> {
    let text = text.trim();
    let text = match text.get(..ID_PREFIX.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(ID_PREFIX) => text[ID_PREFIX.len()..].trim(),
        _ => text,
    };
    if text.is_empty() {
        bail!("the identifier is empty");
    }
    EndpointId::from_str(text).map_err(|_| anyhow!("this is not a valid identifier"))
}

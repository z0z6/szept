//! The interface of the messenger for graphical applications.
//!
//! UniFFI turns everything exported here into Kotlin. All calls are blocking
//! and quick, except [`App::start`], [`App::stop`], file offers and archives,
//! which may take a moment: call those off the main thread.

mod archive;
mod engine;
mod passwords;
mod wallet;

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
use komunikator_nano::{Account, Seed, format_xno, hex, parse_xno};
use tokio::{
    runtime::Runtime,
    sync::{Notify, mpsc},
    task::JoinHandle,
};

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
    /// The contact turned their Nano wallet on or off. Read the address
    /// with [`App::contact_wallet`].
    ContactWalletChanged {
        peer: String,
    },
    /// The contact says they paid us. Show it as unchecked until
    /// [`ChatEvent::PaymentChecked`] says "confirmed".
    Payment {
        peer: String,
        id: String,
        sent_at_ms: u64,
        /// The hash of the send block, in hex: look it up in any explorer.
        block: String,
        amount_raw: String,
        /// In XNO, such as "1.5".
        amount: String,
    },
    /// What the ledger says about the payment `id`: "confirmed",
    /// "pending" (sent, not confirmed yet), "invalid" (no such send of that
    /// amount to us: do not trust it), "unverified" (no Nano node answered)
    /// or "no-wallet" (our wallet is off).
    PaymentChecked {
        peer: String,
        id: String,
        status: String,
    },
    /// Money arrived in our wallet and was received.
    NanoReceived {
        payment: ReceivedPayment,
    },
    /// The wallet's balance may have changed: read it again.
    WalletChanged,
}

/// A file just offered to a contact or a group.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SentFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub hash: String,
}

/// The state of our Nano wallet.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WalletStatus {
    /// A seed exists on this device, whether the wallet is on or not.
    pub exists: bool,
    pub enabled: bool,
    /// The person proved they wrote the 24 words down. Until then, show a
    /// reminder: without the words, a lost phone means lost money.
    pub backed_up: bool,
    /// Our `nano_` address, when a seed exists.
    pub address: Option<String>,
    /// The Nano nodes in use, in order.
    pub nodes: Vec<String>,
}

/// Our balance: confirmed, and sent to us but not yet received.
#[derive(Debug, Clone, uniffi::Record)]
pub struct WalletBalance {
    /// In XNO, such as "1.5".
    pub balance: String,
    pub balance_raw: String,
    pub receivable: String,
    pub receivable_raw: String,
}

/// A payment that reached our wallet.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ReceivedPayment {
    /// The hash of the sender's send block.
    pub block: String,
    pub amount: String,
    pub amount_raw: String,
    /// The sender's address, empty when the node did not say.
    pub from: String,
    /// The contact with that address, if any.
    pub peer: Option<String>,
}

/// A payment that left our wallet.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SentPayment {
    /// The chat message telling the contact, when there is one.
    pub id: Option<String>,
    /// The hash of our send block.
    pub block: String,
    pub amount: String,
    pub amount_raw: String,
    /// Where it went.
    pub address: String,
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

/// Reads an amount in XNO ("1", "0.5", "1,5") and returns it in raw.
#[uniffi::export]
pub fn parse_nano_amount(text: String) -> Result<String> {
    Ok(parse_xno(&text)?.to_string())
}

/// Writes an amount in raw as XNO, such as "1.5".
#[uniffi::export]
pub fn format_nano_amount(raw: String) -> Result<String> {
    let raw: u128 = raw.trim().parse().context("not an amount in raw")?;
    Ok(format_xno(raw))
}

/// Checks a Nano address or `nano:` link (including its checksum, which
/// catches typos) and returns the bare `nano_` address.
#[uniffi::export]
pub fn normalize_nano_address(text: String) -> Result<String> {
    Ok(Account::parse_link(&text)?.to_string())
}

/// The text for a QR code other Nano wallets understand: `nano:<address>`,
/// with an amount in raw when one is given.
#[uniffi::export]
pub fn nano_link(address: String, amount_raw: Option<String>) -> Result<String> {
    let account = Account::parse_link(&address)?;
    Ok(match amount_raw {
        Some(raw) if !raw.trim().is_empty() => {
            let raw: u128 = raw.trim().parse().context("not an amount in raw")?;
            format!("nano:{account}?amount={raw}")
        }
        _ => format!("nano:{account}"),
    })
}

/// The public Nano nodes used when none are set.
#[uniffi::export]
pub fn default_nano_nodes() -> Vec<String> {
    komunikator_nano::rpc::DEFAULT_NODES
        .iter()
        .map(|node| (*node).to_owned())
        .collect()
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
    pub(crate) data_dir: PathBuf,
    /// The Nano wallet, off until the person turns it on.
    pub(crate) wallet: Mutex<wallet::State>,
    /// Contacts' Nano addresses, by identifier.
    peer_wallets: Mutex<BTreeMap<String, String>>,
    /// Wakes the wallet up to receive at once.
    pub(crate) wallet_kick: Arc<Notify>,
}

impl Shared {
    pub(crate) fn active_wallet(&self) -> Option<Arc<komunikator_nano::Wallet>> {
        self.wallet.lock().unwrap().active.clone()
    }

    /// What to tell contacts about our wallet: our account while it is on,
    /// `None` after it was turned off, nothing when there never was one.
    pub(crate) fn wallet_signal(&self) -> Option<Signal> {
        let state = self.wallet.lock().unwrap();
        let account = state.account?;
        Some(Signal::Wallet(
            state.active.is_some().then_some(*account.as_bytes()),
        ))
    }

    /// Stores a contact's Nano account. Returns `true` on a change.
    pub(crate) fn store_peer_wallet(&self, peer: &EndpointId, account: Option<[u8; 32]>) -> bool {
        let mut wallets = self.peer_wallets.lock().unwrap();
        let key = peer.to_string();
        let address = account.map(|bytes| Account(bytes).to_string());
        if wallets.get(&key) == address.as_ref() {
            return false;
        }
        match address {
            Some(address) => wallets.insert(key, address),
            None => wallets.remove(&key),
        };
        let _ = save_json(&wallet::peers_path(&self.data_dir), &*wallets);
        true
    }

    pub(crate) fn peer_wallet(&self, peer: &EndpointId) -> Option<Account> {
        self.peer_wallets
            .lock()
            .unwrap()
            .get(&peer.to_string())
            .and_then(|address| address.parse().ok())
    }

    /// The contact whose wallet has `address`.
    pub(crate) fn peer_with_wallet(&self, address: &str) -> Option<String> {
        self.peer_wallets
            .lock()
            .unwrap()
            .iter()
            .find(|(_, known)| known.as_str() == address)
            .map(|(peer, _)| peer.clone())
    }

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
        self.store_peer_wallet(peer, None);
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
    receiver: JoinHandle<()>,
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
        let wallet = wallet::State::load(&data_dir)?;
        let peer_wallets = wallet::load_peers(&data_dir);
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
                data_dir: data_dir.clone(),
                wallet: Mutex::new(wallet),
                peer_wallets: Mutex::new(peer_wallets),
                wallet_kick: Arc::new(Notify::new()),
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
        let receiver = self.runtime.spawn(wallet::receive_loop(
            self.shared.clone(),
            listener.clone(),
            self.shared.wallet_kick.clone(),
        ));
        *running = Some(Running {
            node,
            commands,
            engine,
            receiver,
            listener,
        });
        Ok(())
    }

    /// Goes offline. Messages still waiting in the queue are dropped.
    pub fn stop(&self) {
        if let Some(running) = self.running.lock().unwrap().take() {
            running.engine.abort();
            running.receiver.abort();
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
    /// Other files are listed, not included. The Nano wallet seed, when
    /// there is one, always goes in: money must survive a lost phone.
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
        let wallet = wallet::read_seed(&self.shared.data_dir)?.map(|seed| *seed.bytes());
        archive::export(
            &self.shared,
            include_identity.then(|| self.identity.secret_bytes()),
            wallet,
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

    /// The state of our Nano wallet. Quick: reads nothing from the network.
    pub fn wallet_status(&self) -> WalletStatus {
        let state = self.shared.wallet.lock().unwrap();
        let nodes = wallet::rpc(&state.settings)
            .map(|rpc| rpc.nodes().to_vec())
            .unwrap_or_default();
        WalletStatus {
            exists: state.account.is_some(),
            enabled: state.active.is_some(),
            backed_up: state.settings.backed_up,
            address: state.account.map(|account| account.to_string()),
            nodes,
        }
    }

    /// Turns the wallet on. The first time, this creates a new seed; after
    /// that, it opens the one already on this device. A seed is never
    /// replaced here. Contacts learn our address on the next connection.
    /// Show [`App::wallet_words`] right after the first time.
    pub fn wallet_create(&self) -> Result<WalletStatus> {
        let data_dir = &self.shared.data_dir;
        {
            let mut state = self.shared.wallet.lock().unwrap();
            if wallet::read_seed(data_dir)?.is_none() {
                wallet::store_seed(data_dir, &Seed::generate())?;
                state.settings.backed_up = false;
            }
            state.open(data_dir)?;
        }
        self.wallet_announced();
        Ok(self.wallet_status())
    }

    /// The 24 words that restore the wallet here or in any Nano wallet
    /// (Nault, Natrium…). Show them only on request, never in a log.
    pub fn wallet_words(&self) -> Result<Vec<String>> {
        let seed = wallet::read_seed(&self.shared.data_dir)?.context("there is no wallet yet")?;
        Ok(seed.words().iter().map(|word| word.to_string()).collect())
    }

    /// Three different positions (1 to 24) to ask about after showing the
    /// words, to check they were written down.
    pub fn wallet_backup_quiz(&self) -> Vec<u32> {
        let mut positions: Vec<u32> = Vec::new();
        while positions.len() < 3 {
            let position = (rand::random::<u32>() % 24) + 1;
            if !positions.contains(&position) {
                positions.push(position);
            }
        }
        positions.sort_unstable();
        positions
    }

    /// Checks the words given for `positions` (1 to 24, as from
    /// [`App::wallet_backup_quiz`]). On a match the backup counts as done.
    pub fn wallet_confirm_backup(&self, positions: Vec<u32>, words: Vec<String>) -> Result<bool> {
        if positions.is_empty() || positions.len() != words.len() {
            return Err(anyhow!("give one word for each position").into());
        }
        let real = self.wallet_words()?;
        let matches = positions.iter().zip(&words).all(|(position, word)| {
            let index = (*position as usize).wrapping_sub(1);
            real.get(index)
                .is_some_and(|real| real.eq_ignore_ascii_case(word.trim()))
        });
        if matches {
            let mut state = self.shared.wallet.lock().unwrap();
            state.settings.backed_up = true;
            wallet::save_settings(&self.shared.data_dir, &state.settings)?;
        }
        Ok(matches)
    }

    /// Restores a wallet from its 24 words (or the seed in hex) and turns it
    /// on. A different seed that was here is kept aside in the data
    /// directory as `wallet-<time>.seed.old`, never deleted.
    pub fn wallet_restore(&self, backup: String) -> Result<WalletStatus> {
        let seed = Seed::parse(&backup)?;
        let data_dir = &self.shared.data_dir;
        {
            let mut state = self.shared.wallet.lock().unwrap();
            wallet::store_seed(data_dir, &seed)?;
            state.settings.backed_up = true;
            state.open(data_dir)?;
        }
        self.wallet_announced();
        self.shared.wallet_kick.notify_one();
        Ok(self.wallet_status())
    }

    /// Turns the wallet off: contacts stop seeing our address and nothing
    /// is received automatically. The seed stays, and so does the money:
    /// [`App::wallet_create`] turns the same wallet back on.
    pub fn wallet_disable(&self) -> Result<()> {
        {
            let mut state = self.shared.wallet.lock().unwrap();
            state.active = None;
            state.settings.enabled = false;
            wallet::save_settings(&self.shared.data_dir, &state.settings)?;
        }
        self.wallet_announced();
        Ok(())
    }

    /// Sets the Nano nodes to use, in order (`https://`, or `http://` on
    /// this machine or the local network). An empty list means the defaults.
    pub fn wallet_set_nodes(&self, nodes: Vec<String>) -> Result<()> {
        let mut state = self.shared.wallet.lock().unwrap();
        let mut settings = state.settings.clone();
        settings.nodes = nodes
            .into_iter()
            .map(|node| node.trim().to_owned())
            .filter(|node| !node.is_empty())
            .collect();
        wallet::rpc(&settings)?;
        state.settings = settings;
        wallet::save_settings(&self.shared.data_dir, &state.settings)?;
        if state.active.is_some() {
            state.open(&self.shared.data_dir)?;
        }
        Ok(())
    }

    /// Our balance, from a Nano node. Blocks for up to half a minute: call
    /// off the main thread.
    pub fn wallet_balance(&self) -> Result<WalletBalance> {
        let wallet = self.active_wallet()?;
        let balance = self.runtime.block_on(wallet.balance())?;
        Ok(WalletBalance {
            balance: format_xno(balance.balance),
            balance_raw: balance.balance.to_string(),
            receivable: format_xno(balance.receivable),
            receivable_raw: balance.receivable.to_string(),
        })
    }

    /// Receives what was sent to us right now, instead of waiting for the
    /// next automatic round. Each payment is also reported as
    /// [`ChatEvent::NanoReceived`]. Slow: call off the main thread.
    pub fn wallet_receive(&self) -> Result<Vec<ReceivedPayment>> {
        let wallet = self.active_wallet()?;
        let listener = self.listener();
        Ok(self.runtime.block_on(async {
            match &listener {
                Some(listener) => {
                    wallet::receive_now(&self.shared, &wallet, listener.as_ref()).await
                }
                None => wallet::receive_now(&self.shared, &wallet, &Silent).await,
            }
        }))
    }

    /// The Nano address the contact `peer` shared with us, if their wallet is on.
    pub fn contact_wallet(&self, peer: String) -> Option<String> {
        let peer = parse_id(&peer).ok()?;
        self.shared
            .peer_wallet(&peer)
            .map(|account| account.to_string())
    }

    /// Pays the contact `to` `amount` XNO (such as "0.5") to the address
    /// they shared, then tells them in the chat. Money cannot be taken
    /// back: ask the person to confirm the amount and the recipient first.
    ///
    /// Computing the proof-of-work can take from a moment up to a minute or
    /// two on a slow phone. Call off the main thread. The app must be online.
    pub fn send_nano(&self, to: String, amount: String) -> Result<SentPayment> {
        let peer = parse_id(&to)?;
        if !self.shared.is_contact(&peer) {
            return Err(anyhow!("payments go only to contacts").into());
        }
        let account = self
            .shared
            .peer_wallet(&peer)
            .context("this contact has no Nano wallet turned on")?;
        // The chat note must be able to follow the money.
        self.node()?;
        let mut sent = self.pay(account, &amount)?;
        let amount_raw: u128 = sent.amount_raw.parse().context("bad amount")?;
        let block: [u8; 32] = komunikator_nano::from_hex(&sent.block)?;
        let message = Outgoing::post(
            None,
            Post::Payment {
                block,
                amount: amount_raw,
            },
        )?;
        // The money already left: a failure here must not look like a
        // failed payment.
        sent.id = self.queue_message(&peer.to_string(), message).ok();
        Ok(sent)
    }

    /// Pays `amount` XNO to any Nano address or `nano:` link, for example
    /// to move money to another wallet or an exchange. Nobody is told in
    /// the chat. Slow: call off the main thread.
    pub fn send_nano_to_address(&self, address: String, amount: String) -> Result<SentPayment> {
        let account = Account::parse_link(&address)?;
        self.pay(account, &amount)
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

/// Listens to nothing, for calls made while the app is offline.
struct Silent;

impl EventListener for Silent {
    fn on_event(&self, _event: ChatEvent) {}
}

impl App {
    fn active_wallet(&self) -> Result<Arc<komunikator_nano::Wallet>> {
        self.shared
            .active_wallet()
            .ok_or_else(|| anyhow!("the Nano wallet is off").into())
    }

    fn listener(&self) -> Option<Arc<dyn EventListener>> {
        let running = self.running.lock().unwrap();
        running.as_ref().map(|running| running.listener.clone())
    }

    fn pay(&self, account: Account, amount: &str) -> Result<SentPayment> {
        let wallet = self.active_wallet()?;
        let raw = parse_xno(amount)?;
        let block = self.runtime.block_on(wallet.send(account, raw))?;
        if let Some(listener) = self.listener() {
            listener.on_event(ChatEvent::WalletChanged);
        }
        Ok(SentPayment {
            id: None,
            block: hex(&block),
            amount: format_xno(raw),
            amount_raw: raw.to_string(),
            address: account.to_string(),
        })
    }

    /// Tells everyone connected what changed about our wallet.
    fn wallet_announced(&self) {
        let _ = self.command(Command::ProfileChanged);
    }

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

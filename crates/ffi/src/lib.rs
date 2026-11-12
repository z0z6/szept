//! The interface of the messenger for graphical applications.
//!
//! UniFFI turns everything exported here into Kotlin. All calls are blocking
//! and quick, except [`App::start`] and [`App::stop`], which may take a
//! moment: call those two off the main thread.

mod engine;

use std::{
    collections::{HashMap, HashSet},
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
    Contacts, Content, EndpointAddr, EndpointId, Event, FileOffer, Identity, MessageId, Network,
    Node, Outgoing, contacts, files, identity,
    protocol::{MAX_AVATAR_LEN, MAX_MEDIA_SIZE},
};
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

use crate::engine::Command;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
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

/// Something that happened on the network. `peer` is the contact's identifier.
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
    },
    /// The contact's device confirmed the receipt of our message `id`.
    Delivered {
        peer: String,
        id: String,
    },
    /// Our message `id` left this device. [`ChatEvent::Delivered`] follows
    /// once the contact's device confirms it.
    Sent {
        peer: String,
        id: String,
    },
    /// The contact offers a file. Fetch it with [`App::download`].
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
    },
    /// The contact set, changed or removed their profile picture. Read it
    /// with [`App::avatar`].
    AvatarChanged {
        peer: String,
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
}

/// A file just offered to a contact.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SentFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub hash: String,
}

impl From<Event> for ChatEvent {
    fn from(event: Event) -> Self {
        match event {
            Event::Connected { peer } => Self::Connected {
                peer: peer.to_string(),
            },
            Event::Disconnected { peer } => Self::Disconnected {
                peer: peer.to_string(),
            },
            Event::Message {
                peer,
                id,
                sent_at_ms,
                body,
            } => Self::Message {
                peer: peer.to_string(),
                id: id.to_string(),
                sent_at_ms,
                body,
            },
            Event::Delivered { peer, id } => Self::Delivered {
                peer: peer.to_string(),
                id: id.to_string(),
            },
            Event::FileOffered {
                peer,
                id,
                sent_at_ms,
                offer,
                media,
            } => Self::FileOffered {
                peer: peer.to_string(),
                id: id.to_string(),
                sent_at_ms,
                hash: files::hash_hex(&offer.hash),
                name: offer.name,
                size: offer.size,
                media: media.map(Media::from),
            },
            Event::Avatar { peer, .. } => Self::AvatarChanged {
                peer: peer.to_string(),
            },
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
    contacts: Mutex<Contacts>,
    network: Network,
    running: Mutex<Option<Running>>,
    shared: Arc<Shared>,
}

/// What the commands and the background engine both use.
pub(crate) struct Shared {
    /// Files contacts offered us, by message id.
    pub(crate) offers: Mutex<HashMap<String, (EndpointId, MessageId, FileOffer)>>,
    /// Message ids of the downloads in progress.
    pub(crate) downloading: Mutex<HashSet<String>>,
    /// Where voice messages and GIFs from contacts land.
    pub(crate) media_dir: PathBuf,
    /// Contacts' profile pictures, one file per identifier.
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

    /// Stores what a contact sent as their picture. Returns `true` if it
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
/// crash never leaves half a picture behind.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let part = path.with_extension("part");
    fs::write(&part, bytes)?;
    fs::rename(&part, path)
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
        let avatar_path = data_dir.join("avatar.img");
        let avatar = fs::read(&avatar_path)
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
            identity,
            contacts: Mutex::new(contacts),
            network: if local_only {
                Network::LocalOnly
            } else {
                Network::Public
            },
            running: Mutex::new(None),
            shared: Arc::new(Shared {
                offers: Mutex::default(),
                downloading: Mutex::default(),
                media_dir: data_dir.join("media"),
                avatars_dir: data_dir.join("avatars"),
                avatar_path,
                avatar: Mutex::new(avatar),
            }),
        }))
    }

    /// The identifier to hand to other people.
    pub fn my_id(&self) -> String {
        self.identity.id().to_string()
    }

    /// The address book, sorted by alias.
    pub fn contacts(&self) -> Vec<Contact> {
        let contacts = self.contacts.lock().unwrap();
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
        self.contacts.lock().unwrap().add(&alias, id)?;
        if let Some(running) = &*self.running.lock().unwrap() {
            running.node.allow(id);
            let _ = running.commands.send(Command::ContactAdded(id));
        }
        Ok(())
    }

    /// Deletes a contact and closes the connection with it.
    pub fn remove_contact(&self, alias: String) -> Result<()> {
        let id = {
            let mut contacts = self.contacts.lock().unwrap();
            let id = contacts
                .get(&alias)
                .with_context(|| format!("unknown contact `{alias}`"))?;
            contacts.remove(&alias)?;
            id
        };
        let _ = fs::remove_file(self.shared.avatar_file(&id));
        if let Some(running) = &*self.running.lock().unwrap() {
            running.node.disallow(&id);
            let _ = running.commands.send(Command::ContactRemoved(id));
        }
        Ok(())
    }

    /// Goes online and reports everything that happens to `listener`.
    pub fn start(&self, listener: Arc<dyn EventListener>) -> Result<()> {
        let mut running = self.running.lock().unwrap();
        if running.is_some() {
            return Ok(());
        }
        let known: Vec<EndpointId> = {
            let contacts = self.contacts.lock().unwrap();
            contacts.iter().map(|(_, id)| id).collect()
        };
        let (node, events) = self.runtime.block_on(Node::spawn(
            &self.identity,
            known.iter().copied(),
            self.network,
        ))?;
        let (commands, commands_rx) = mpsc::unbounded_channel();
        let engine = self.runtime.spawn(engine::run(
            node.clone(),
            events,
            commands_rx,
            listener.clone(),
            self.shared.clone(),
            known,
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

    /// Queues a text message for a contact and returns its identifier.
    ///
    /// The message leaves as soon as the contact is reachable.
    /// [`ChatEvent::Delivered`] with the same identifier confirms the receipt.
    pub fn send(&self, peer: String, body: String) -> Result<String> {
        let peer = parse_id(&peer)?;
        let message = Outgoing::new(body)?;
        let id = message.id.to_string();
        self.command(Command::Send { peer, message })?;
        Ok(id)
    }

    /// Offers the file at `path` to a contact and returns what was offered.
    ///
    /// The file stays where it is: the contact fetches it straight from this
    /// device, so it must not move or change until then, and this device must
    /// be online. Reads the whole file to hash it: call off the main thread,
    /// and not from inside an async runtime.
    pub fn send_file(&self, peer: String, path: String) -> Result<SentFile> {
        let peer = parse_id(&peer)?;
        let node = self.node()?;
        let message = self.runtime.block_on(node.share(peer, Path::new(&path)))?;
        self.offer(peer, message)
    }

    /// Sends a voice message or a GIF stored at `path`. Works like
    /// [`App::send_file`], except that the contact's app fetches it at once.
    ///
    /// Voice messages must be `.m4a`, `.aac`, `.ogg`, `.oga`, `.opus`,
    /// `.webm`, `.mp3` or `.wav`, GIFs `.gif`, `.webp`, `.png`, `.jpg` or
    /// `.jpeg`, both at most [`max_media_size`] bytes.
    pub fn send_media(&self, peer: String, path: String, media: Media) -> Result<SentFile> {
        let peer = parse_id(&peer)?;
        let node = self.node()?;
        let message =
            self.runtime
                .block_on(node.share_media(peer, Path::new(&path), media.into()))?;
        self.offer(peer, message)
    }

    /// Our profile picture, if there is one.
    pub fn my_avatar(&self) -> Option<Vec<u8>> {
        let avatar = self.shared.avatar.lock().unwrap();
        (!avatar.is_empty()).then(|| avatar.clone())
    }

    /// Sets our profile picture, or removes it when `image` is empty, and
    /// sends it to every contact: right away to the connected ones, on the
    /// next connection to the others.
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
        let _ = self.command(Command::AvatarChanged);
        Ok(())
    }

    /// The profile picture a contact sent us, if any. Arrives with
    /// [`ChatEvent::AvatarChanged`] and stays after a restart.
    pub fn avatar(&self, peer: String) -> Option<Vec<u8>> {
        let peer = parse_id(&peer).ok()?;
        fs::read(self.shared.avatar_file(&peer))
            .ok()
            .filter(|image| image.len() <= MAX_AVATAR_LEN && files::is_picture(image))
    }

    /// Where voice messages and GIFs from contacts are saved.
    pub fn media_dir(&self) -> String {
        self.shared.media_dir.to_string_lossy().into_owned()
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

    /// Call when the device switches networks (Wi-Fi, mobile data, none):
    /// the app then finds its new addresses and reconnects at once instead
    /// of at the next periodic retry. Harmless when nothing changed.
    pub fn network_changed(&self) {
        let _ = self.command(Command::NetworkChanged);
    }

    /// Returns `true` if a connection with the contact is up.
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

impl App {
    /// Queues a file offer made by the node and describes it.
    fn offer(&self, peer: EndpointId, message: Outgoing) -> Result<SentFile> {
        let sent = match &message.content {
            Content::File(offer) | Content::Media(offer, _) => SentFile {
                id: message.id.to_string(),
                name: offer.name.clone(),
                size: offer.size,
                hash: files::hash_hex(&offer.hash),
            },
            Content::Text(_) => unreachable!("share makes a file offer"),
        };
        self.command(Command::Send { peer, message })?;
        Ok(sent)
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

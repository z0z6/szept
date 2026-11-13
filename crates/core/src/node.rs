//! The network node: accepts connections from contacts, dials them and moves
//! frames in both directions.
//!
//! Connections are direct QUIC connections between two devices, addressed by
//! public key. Both sides prove their key in the TLS 1.3 handshake, so the
//! connection is encrypted end to end even when a relay forwards the packets.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::SeekFrom,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use iroh::{
    Endpoint, EndpointAddr, EndpointId, RelayMode,
    endpoint::{Connection, RecvStream, SendStream, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::mpsc,
    time::timeout,
};
use tracing::debug;

use crate::{
    files,
    identity::Identity,
    protocol::{
        ALPN, FETCH_GONE, FETCH_OK, FETCH_UNKNOWN, FetchRequest, FileOffer, Frame, GroupId,
        GroupInfo, MAX_AVATAR_LEN, MAX_FRAME_LEN, MAX_MEDIA_SIZE, Media, MessageId, Outgoing, Post,
        Signal,
    },
};

/// How long a dial may take before the contact counts as offline.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a peer may take to deliver one frame after opening its stream.
const FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// QUIC application error code sent to peers that are not in the address book.
const CLOSE_NOT_A_CONTACT: u32 = 1;

/// QUIC application error code sent when a contact is removed.
const CLOSE_CONTACT_REMOVED: u32 = 2;

/// Number of events buffered for the user interface.
const EVENT_BUFFER: usize = 256;

/// How long the sending side may stay silent during a file transfer.
const TRANSFER_STALL: Duration = Duration::from_secs(60);

/// Size of one read or write during a file transfer.
const CHUNK: usize = 256 * 1024;

/// Which outside services the node may use to find and reach contacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// Uses the public address lookup and relay servers run by the iroh
    /// project. They help two devices find each other and get through NAT.
    /// They see who connects to whom, never the content.
    Public,
    /// Uses nothing outside this machine or local network. Contacts must be
    /// dialled with a full address. Used by the tests.
    LocalOnly,
}

/// Something that happened on the network, reported to the user interface.
#[derive(Debug, Clone)]
pub enum Event {
    /// A connection with a contact is up.
    Connected { peer: EndpointId },
    /// The connection with a contact is gone.
    Disconnected { peer: EndpointId },
    /// A contact sent a text message.
    Message {
        peer: EndpointId,
        id: MessageId,
        sent_at_ms: u64,
        body: String,
    },
    /// A contact's device confirmed that it received our message.
    Delivered { peer: EndpointId, id: MessageId },
    /// A contact offers a file. Fetch it with [`Node::download`].
    ///
    /// With `media` set it is a voice message or a GIF, meant to be fetched
    /// at once and shown inside the conversation.
    FileOffered {
        peer: EndpointId,
        id: MessageId,
        sent_at_ms: u64,
        offer: FileOffer,
        media: Option<Media>,
    },
    /// A contact sent their profile picture: a JPEG, PNG, WebP or GIF of at
    /// most [`MAX_AVATAR_LEN`] bytes, or nothing when they removed it.
    Avatar { peer: EndpointId, image: Vec<u8> },
    /// A contact sent a post: a message for a group, or a sticker. A file
    /// in a post is fetched with [`Node::download`] like any other.
    Post {
        peer: EndpointId,
        id: MessageId,
        sent_at_ms: u64,
        group: Option<GroupId>,
        post: Post,
    },
    /// A contact reacted to the message `target`; empty means taken back.
    Reaction {
        peer: EndpointId,
        target: MessageId,
        group: Option<GroupId>,
        reaction: String,
    },
    /// A group's owner sent its state. Check that `peer` is the owner.
    Group { peer: EndpointId, info: GroupInfo },
    /// A contact left a group.
    Left { peer: EndpointId, group: GroupId },
    /// A contact sent their display name.
    Name { peer: EndpointId, name: String },
}

/// A file this device offered to a contact.
#[derive(Debug, Clone)]
struct Share {
    /// Who may fetch it: one contact, or the members of a group.
    peers: Vec<EndpointId>,
    path: PathBuf,
    offer: FileOffer,
}

/// A running node. Cloning is cheap and gives another handle to the same node.
#[derive(Clone)]
pub struct Node {
    inner: Arc<Inner>,
    router: Router,
}

struct Inner {
    endpoint: Endpoint,
    allowed: RwLock<HashSet<EndpointId>>,
    /// The connection used for sending to each contact.
    connections: Mutex<HashMap<EndpointId, Connection>>,
    /// Files offered to contacts, by the id of the offering message.
    shares: Mutex<HashMap<MessageId, Share>>,
    events: mpsc::Sender<Event>,
}

impl Node {
    /// Starts a node that talks only to `contacts`.
    ///
    /// Returns the node and the stream of its events. Must be called from
    /// inside a tokio runtime.
    pub async fn spawn(
        identity: &Identity,
        contacts: impl IntoIterator<Item = EndpointId>,
        network: Network,
    ) -> Result<(Self, mpsc::Receiver<Event>)> {
        let builder = match network {
            Network::Public => Endpoint::builder(presets::N0),
            Network::LocalOnly => {
                Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled)
            }
        };
        let endpoint = builder
            .crypto_provider(crate::crypto::provider())
            .secret_key(identity.secret_key())
            .bind()
            .await
            .context("cannot start the network endpoint")?;

        let (events, events_rx) = mpsc::channel(EVENT_BUFFER);
        let inner = Arc::new(Inner {
            endpoint: endpoint.clone(),
            allowed: RwLock::new(contacts.into_iter().collect()),
            connections: Mutex::default(),
            shares: Mutex::default(),
            events,
        });
        let router = Router::builder(endpoint)
            .accept(ALPN, ChatProtocol(inner.clone()))
            .spawn();
        Ok((Self { inner, router }, events_rx))
    }

    /// The public identifier of this node.
    pub fn id(&self) -> EndpointId {
        self.inner.endpoint.id()
    }

    /// The full address of this node: its identifier plus the network
    /// addresses currently known for it.
    pub fn addr(&self) -> EndpointAddr {
        self.inner.endpoint.addr()
    }

    /// The local sockets this node listens on.
    pub fn bound_sockets(&self) -> Vec<SocketAddr> {
        self.inner.endpoint.bound_sockets()
    }

    /// Waits until the node is reachable from the internet, for at most
    /// `limit`. Returns `false` if the limit passed first.
    pub async fn wait_online(&self, limit: Duration) -> bool {
        timeout(limit, self.inner.endpoint.online()).await.is_ok()
    }

    /// Tells the node the device switched networks, for example from Wi-Fi
    /// to mobile data, so it finds its new addresses right away.
    pub async fn network_changed(&self) {
        self.inner.endpoint.network_change().await;
    }

    /// Lets `peer` connect to this node from now on.
    pub fn allow(&self, peer: EndpointId) {
        self.inner.allowed.write().unwrap().insert(peer);
    }

    /// Stops accepting `peer` and closes the connection with it.
    pub fn disallow(&self, peer: &EndpointId) {
        self.inner.allowed.write().unwrap().remove(peer);
        if let Some(conn) = self.inner.connections.lock().unwrap().remove(peer) {
            conn.close(CLOSE_CONTACT_REMOVED.into(), b"contact removed");
        }
    }

    /// Returns `true` if a connection with `peer` is up.
    pub fn is_connected(&self, peer: &EndpointId) -> bool {
        self.inner.live_connection(peer).is_some()
    }

    /// Connects to a contact unless a connection is already up.
    ///
    /// Pass an [`EndpointId`] to let the address lookup find the contact, or
    /// an [`EndpointAddr`] to dial known network addresses.
    pub async fn connect(&self, addr: impl Into<EndpointAddr>) -> Result<()> {
        let addr = addr.into();
        if self.is_connected(&addr.id) {
            return Ok(());
        }
        self.dial(addr).await.map(drop)
    }

    /// Sends a message to a contact, connecting first if necessary.
    ///
    /// Success means the message left this device. The contact's device
    /// confirms the receipt separately with [`Event::Delivered`].
    pub async fn send(&self, peer: EndpointId, message: &Outgoing) -> Result<()> {
        let conn = match self.inner.live_connection(&peer) {
            Some(conn) => conn,
            None => self.dial(peer.into()).await?,
        };
        write_frame(&conn, &message.to_frame()).await
    }

    /// Makes the file at `path` available to `peer` and returns the message
    /// that offers it. Send that message like any other.
    ///
    /// The file is read once here to compute its hash, and again whenever the
    /// contact fetches it. Until then it must stay where it is, unchanged.
    pub async fn share(&self, peer: EndpointId, path: &Path) -> Result<Outgoing> {
        self.share_as(peer, path, None).await
    }

    /// Like [`Node::share`], for a voice message or a GIF. The file must be
    /// at most [`MAX_MEDIA_SIZE`] bytes and of a type that fits `media`.
    pub async fn share_media(
        &self,
        peer: EndpointId,
        path: &Path,
        media: Media,
    ) -> Result<Outgoing> {
        self.share_as(peer, path, Some(media)).await
    }

    /// Offers a file, or a voice message or GIF when `media` is set, as a
    /// post that every one of `peers` may fetch: the members of `group`.
    pub async fn share_post(
        &self,
        peers: Vec<EndpointId>,
        group: Option<GroupId>,
        path: &Path,
        media: Option<Media>,
    ) -> Result<Outgoing> {
        let path = path.to_owned();
        let offer = tokio::task::spawn_blocking({
            let path = path.clone();
            move || files::describe(&path)
        })
        .await??;
        if let Some(media) = media {
            ensure!(offer.size <= MAX_MEDIA_SIZE, "the media file is too large");
            ensure!(
                media.accepts(&offer.name),
                "this file cannot be sent as media"
            );
        }
        let message = Outgoing::post_unchecked(
            group,
            Post::File {
                offer: offer.clone(),
                media,
            },
        );
        self.inner
            .shares
            .lock()
            .unwrap()
            .insert(message.id, Share { peers, path, offer });
        Ok(message)
    }

    /// Sends a reaction, a group change or a name to a contact, connecting
    /// first if necessary.
    pub async fn signal(&self, peer: EndpointId, signal: &Signal) -> Result<()> {
        let conn = match self.inner.live_connection(&peer) {
            Some(conn) => conn,
            None => self.dial(peer.into()).await?,
        };
        write_frame(&conn, &signal.to_frame()).await
    }

    async fn share_as(
        &self,
        peer: EndpointId,
        path: &Path,
        media: Option<Media>,
    ) -> Result<Outgoing> {
        let path = path.to_owned();
        let offer = tokio::task::spawn_blocking({
            let path = path.clone();
            move || files::describe(&path)
        })
        .await??;
        let message = match media {
            None => Outgoing::file(offer.clone()),
            Some(media) => {
                ensure!(offer.size <= MAX_MEDIA_SIZE, "the media file is too large");
                ensure!(
                    media.accepts(&offer.name),
                    "this file cannot be sent as media"
                );
                Outgoing::media(offer.clone(), media)
            }
        };
        self.inner.shares.lock().unwrap().insert(
            message.id,
            Share {
                peers: vec![peer],
                path,
                offer,
            },
        );
        Ok(message)
    }

    /// Sends our profile picture to a contact that is connected right now.
    /// An empty `image` tells the contact we have none. Fails, sending
    /// nothing, while the contact is offline: every new connection should
    /// start with this call.
    pub async fn send_avatar(&self, peer: EndpointId, image: &[u8]) -> Result<()> {
        ensure!(image.len() <= MAX_AVATAR_LEN, "the picture is too large");
        ensure!(
            image.is_empty() || files::is_picture(image),
            "this is not a JPEG, PNG, WebP or GIF picture"
        );
        let conn = self
            .inner
            .live_connection(&peer)
            .context("the contact is not connected")?;
        let frame = Frame::Avatar {
            image: image.to_vec(),
        };
        write_frame(&conn, &frame).await
    }

    /// Stops offering the file sent as `id`.
    pub fn unshare(&self, id: &MessageId) {
        self.inner.shares.lock().unwrap().remove(id);
    }

    /// Fetches the file `peer` offered as `id` into `dir` and returns where it
    /// landed. Never overwrites an existing file.
    ///
    /// An interrupted download resumes from where it stopped the next time it
    /// is called. Nothing is kept unless the content matches the offered
    /// hash. `progress` gets the number of bytes received so far.
    pub async fn download(
        &self,
        peer: EndpointId,
        id: MessageId,
        offer: &FileOffer,
        dir: &Path,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<PathBuf> {
        ensure!(files::is_safe_name(&offer.name), "unsafe file name");
        fs::create_dir_all(dir)
            .await
            .with_context(|| format!("cannot create {}", dir.display()))?;
        let part = dir.join(format!(".{id}.szept-part"));

        // Resume: hash what is already there, then ask for the rest.
        let mut hasher = blake3::Hasher::new();
        let mut have = match fs::metadata(&part).await {
            Ok(metadata) if metadata.len() <= offer.size => metadata.len(),
            _ => 0,
        };
        if have > 0 {
            let part = part.clone();
            let (resumed, state) = tokio::task::spawn_blocking(move || {
                let mut file = std::fs::File::open(&part)?;
                let mut hasher = blake3::Hasher::new();
                let read = files::copy_into_hasher(&mut file, &mut hasher)?;
                Ok::<_, std::io::Error>((read, hasher))
            })
            .await??;
            have = resumed;
            hasher = state;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(have == 0)
            .open(&part)
            .await
            .with_context(|| format!("cannot write {}", part.display()))?;
        file.seek(SeekFrom::Start(have)).await?;
        progress(have);

        if have < offer.size {
            let conn = match self.inner.live_connection(&peer) {
                Some(conn) => conn,
                None => self.dial(peer.into()).await?,
            };
            let (mut send, mut recv) = conn.open_bi().await.context("connection lost")?;
            let request = postcard::to_stdvec(&FetchRequest { id, offset: have })?;
            send.write_all(&request).await.context("connection lost")?;
            send.finish().context("connection lost")?;

            let mut status = [0u8; 1];
            timeout(TRANSFER_STALL, recv.read_exact(&mut status))
                .await
                .context("the contact did not answer in time")?
                .context("the contact closed the transfer")?;
            match status[0] {
                FETCH_OK => {}
                FETCH_UNKNOWN => bail!("the contact no longer shares this file"),
                FETCH_GONE => bail!("the file changed or disappeared on the contact's device"),
                other => bail!("unexpected answer {other}"),
            }

            let mut buffer = vec![0u8; CHUNK];
            loop {
                let read = timeout(TRANSFER_STALL, recv.read(&mut buffer))
                    .await
                    .context("the transfer stalled")?
                    .context("connection lost during the transfer")?;
                let Some(read) = read else { break };
                ensure!(
                    have + read as u64 <= offer.size,
                    "the contact sent more than announced"
                );
                file.write_all(&buffer[..read]).await?;
                hasher.update(&buffer[..read]);
                have += read as u64;
                progress(have);
            }
        }
        file.flush().await?;
        file.sync_all().await?;
        drop(file);

        ensure!(have == offer.size, "the transfer was cut short");
        if *hasher.finalize().as_bytes() != offer.hash {
            let _ = fs::remove_file(&part).await;
            bail!("the file arrived damaged and was deleted");
        }
        let target = files::free_path(dir, &offer.name);
        fs::rename(&part, &target)
            .await
            .with_context(|| format!("cannot save {}", target.display()))?;
        Ok(target)
    }

    /// Closes all connections and stops the node.
    pub async fn shutdown(self) {
        if let Err(err) = self.router.shutdown().await {
            debug!("router shutdown failed: {err}");
        }
    }

    async fn dial(&self, addr: EndpointAddr) -> Result<Connection> {
        let peer = addr.id;
        ensure!(self.inner.is_allowed(&peer), "{peer} is not a contact");
        let conn = timeout(CONNECT_TIMEOUT, self.inner.endpoint.connect(addr, ALPN))
            .await
            .context("the contact did not answer in time")?
            .context("cannot connect to the contact")?;
        self.inner.register(&conn).await;
        tokio::spawn(self.inner.clone().serve_files(conn.clone()));
        tokio::spawn({
            let inner = self.inner.clone();
            let conn = conn.clone();
            async move { inner.read_frames(conn).await }
        });
        Ok(conn)
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node").field("id", &self.id()).finish()
    }
}

impl Inner {
    fn is_allowed(&self, peer: &EndpointId) -> bool {
        self.allowed.read().unwrap().contains(peer)
    }

    fn live_connection(&self, peer: &EndpointId) -> Option<Connection> {
        let connections = self.connections.lock().unwrap();
        connections
            .get(peer)
            .filter(|conn| conn.close_reason().is_none())
            .cloned()
    }

    /// Makes `conn` the connection used for sending to its peer.
    ///
    /// When both sides dial at the same moment, two connections come up. The
    /// older one is deliberately left open: each side may have picked a
    /// different one for sending, and both keep being read.
    async fn register(&self, conn: &Connection) {
        let peer = conn.remote_id();
        let replaced = self.connections.lock().unwrap().insert(peer, conn.clone());
        if replaced.is_none_or(|old| old.close_reason().is_some()) {
            self.emit(Event::Connected { peer }).await;
        }
    }

    /// Reads frames from `conn` until it closes.
    async fn read_frames(&self, conn: Connection) {
        let peer = conn.remote_id();
        while let Ok(mut stream) = conn.accept_uni().await {
            let bytes = match timeout(FRAME_TIMEOUT, stream.read_to_end(MAX_FRAME_LEN)).await {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(err)) => {
                    debug!(%peer, "dropping unreadable frame: {err}");
                    continue;
                }
                Err(_) => {
                    debug!(%peer, "dropping stalled frame");
                    continue;
                }
            };
            match Frame::decode(&bytes) {
                Ok(frame) => self.handle_frame(&conn, peer, frame).await,
                Err(err) => debug!(%peer, "dropping malformed frame: {err}"),
            }
        }

        let was_current = {
            let mut connections = self.connections.lock().unwrap();
            let is_current = connections
                .get(&peer)
                .is_some_and(|current| current.stable_id() == conn.stable_id());
            if is_current {
                connections.remove(&peer);
            }
            is_current
        };
        if was_current {
            self.emit(Event::Disconnected { peer }).await;
        }
    }

    async fn handle_frame(&self, conn: &Connection, peer: EndpointId, frame: Frame) {
        match frame {
            Frame::Text {
                id,
                sent_at_ms,
                body,
            } => {
                self.emit(Event::Message {
                    peer,
                    id,
                    sent_at_ms,
                    body,
                })
                .await;
                if let Err(err) = write_frame(conn, &Frame::Ack { id }).await {
                    debug!(%peer, "cannot confirm message {id}: {err}");
                }
            }
            Frame::Ack { id } => self.emit(Event::Delivered { peer, id }).await,
            Frame::File {
                id,
                sent_at_ms,
                offer,
            } => self.offered(conn, peer, id, sent_at_ms, offer, None).await,
            Frame::Media {
                id,
                sent_at_ms,
                offer,
                media,
            } => {
                self.offered(conn, peer, id, sent_at_ms, offer, Some(media))
                    .await;
            }
            Frame::Avatar { image } => self.emit(Event::Avatar { peer, image }).await,
            Frame::Post {
                id,
                sent_at_ms,
                group,
                post,
            } => {
                self.emit(Event::Post {
                    peer,
                    id,
                    sent_at_ms,
                    group,
                    post,
                })
                .await;
                if let Err(err) = write_frame(conn, &Frame::Ack { id }).await {
                    debug!(%peer, "cannot confirm post {id}: {err}");
                }
            }
            Frame::React {
                target,
                group,
                reaction,
            } => {
                self.emit(Event::Reaction {
                    peer,
                    target,
                    group,
                    reaction,
                })
                .await;
            }
            Frame::Group { info } => self.emit(Event::Group { peer, info }).await,
            Frame::Leave { group } => self.emit(Event::Left { peer, group }).await,
            Frame::Name { name } => self.emit(Event::Name { peer, name }).await,
        }
    }

    async fn offered(
        &self,
        conn: &Connection,
        peer: EndpointId,
        id: MessageId,
        sent_at_ms: u64,
        offer: FileOffer,
        media: Option<Media>,
    ) {
        self.emit(Event::FileOffered {
            peer,
            id,
            sent_at_ms,
            offer,
            media,
        })
        .await;
        if let Err(err) = write_frame(conn, &Frame::Ack { id }).await {
            debug!(%peer, "cannot confirm offer {id}: {err}");
        }
    }

    /// Answers the file requests a contact sends on `conn`.
    async fn serve_files(self: Arc<Self>, conn: Connection) {
        let peer = conn.remote_id();
        while let Ok((send, recv)) = conn.accept_bi().await {
            let inner = self.clone();
            tokio::spawn(async move {
                if let Err(err) = inner.serve_one(peer, send, recv).await {
                    debug!(%peer, "file transfer ended: {err:#}");
                }
            });
        }
    }

    async fn serve_one(
        &self,
        peer: EndpointId,
        mut send: SendStream,
        mut recv: RecvStream,
    ) -> Result<()> {
        let bytes = timeout(FRAME_TIMEOUT, recv.read_to_end(64))
            .await
            .map_err(|_| anyhow!("request timed out"))??;
        let request: FetchRequest = postcard::from_bytes(&bytes)?;
        // Only the contact the file was offered to may fetch it.
        let share = self
            .shares
            .lock()
            .unwrap()
            .get(&request.id)
            .filter(|share| share.peers.contains(&peer))
            .cloned();
        let Some(share) = share else {
            send.write_all(&[FETCH_UNKNOWN]).await?;
            send.finish()?;
            return Ok(());
        };
        let opened = fs::File::open(&share.path).await;
        let mut file = match opened {
            Ok(file) if file.metadata().await?.len() == share.offer.size => file,
            _ => {
                send.write_all(&[FETCH_GONE]).await?;
                send.finish()?;
                return Ok(());
            }
        };
        ensure!(request.offset <= share.offer.size, "offset past the end");
        file.seek(SeekFrom::Start(request.offset)).await?;
        send.write_all(&[FETCH_OK]).await?;
        let mut buffer = vec![0u8; CHUNK];
        loop {
            let read = file.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            send.write_all(&buffer[..read]).await?;
        }
        send.finish()?;
        // Wait until the contact has everything, or gives up.
        let _ = send.stopped().await;
        Ok(())
    }

    async fn emit(&self, event: Event) {
        // The receiver is gone only while the application shuts down.
        let _ = self.events.send(event).await;
    }
}

/// Sends one frame on a fresh unidirectional stream.
async fn write_frame(conn: &Connection, frame: &Frame) -> Result<()> {
    let bytes = frame.encode()?;
    let mut stream = conn.open_uni().await.context("connection lost")?;
    stream.write_all(&bytes).await.context("connection lost")?;
    stream.finish().context("connection lost")?;
    Ok(())
}

/// The accepting side of the chat protocol.
#[derive(Clone)]
struct ChatProtocol(Arc<Inner>);

impl fmt::Debug for ChatProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChatProtocol")
    }
}

impl ProtocolHandler for ChatProtocol {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        // The handshake has finished, so the remote key is proven here.
        if !self.0.is_allowed(&conn.remote_id()) {
            conn.close(CLOSE_NOT_A_CONTACT.into(), b"not a contact");
            return Ok(());
        }
        self.0.register(&conn).await;
        tokio::spawn(self.0.clone().serve_files(conn.clone()));
        self.0.read_frames(conn).await;
        Ok(())
    }
}

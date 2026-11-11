//! The network node: accepts connections from contacts, dials them and moves
//! frames in both directions.
//!
//! Connections are direct QUIC connections between two devices, addressed by
//! public key. Both sides prove their key in the TLS 1.3 handshake, so the
//! connection is encrypted end to end even when a relay forwards the packets.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    net::SocketAddr,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use iroh::{
    Endpoint, EndpointAddr, EndpointId, RelayMode,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use tokio::{sync::mpsc, time::timeout};
use tracing::debug;

use crate::{
    identity::Identity,
    protocol::{ALPN, Frame, MAX_FRAME_LEN, MessageId, Outgoing},
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
            .secret_key(identity.secret_key())
            .bind()
            .await
            .context("cannot start the network endpoint")?;

        let (events, events_rx) = mpsc::channel(EVENT_BUFFER);
        let inner = Arc::new(Inner {
            endpoint: endpoint.clone(),
            allowed: RwLock::new(contacts.into_iter().collect()),
            connections: Mutex::default(),
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
        }
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
        self.0.read_frames(conn).await;
        Ok(())
    }
}

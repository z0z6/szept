//! The interface of the messenger for graphical applications.
//!
//! UniFFI turns everything exported here into Kotlin. All calls are blocking
//! and quick, except [`App::start`] and [`App::stop`], which may take a
//! moment: call those two off the main thread.

mod engine;

use std::{
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use anyhow::{Context, anyhow, bail};
use komunikator_core::{
    Contacts, EndpointAddr, EndpointId, Event, Identity, Network, Node, Outgoing, contacts,
    identity,
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
}

struct Running {
    node: Node,
    commands: mpsc::UnboundedSender<Command>,
    engine: JoinHandle<()>,
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
            listener,
            known,
            // Without a lookup service there is nobody to ask for addresses.
            self.network == Network::Public,
        ));
        *running = Some(Running {
            node,
            commands,
            engine,
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

impl App {
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

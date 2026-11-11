//! Terminal client: a thin layer over `komunikator-core`.

use std::{
    collections::VecDeque,
    io::{self, BufRead},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use komunikator_core::{
    Contacts, EndpointAddr, EndpointId, Event, Identity, Network, Node, Outgoing, contacts,
    identity,
};
use tokio::{sync::mpsc, task::JoinHandle};

/// How often an offline contact is dialled again.
const RETRY_EVERY: Duration = Duration::from_secs(5);

/// How long to wait for the public network at startup.
const ONLINE_LIMIT: Duration = Duration::from_secs(10);

#[derive(Parser)]
#[command(name = "szept", version, about = "Szyfrowany komunikator P2P")]
struct Cli {
    /// Katalog z kluczem i kontaktami (domyślnie: katalog danych użytkownika)
    #[arg(long, global = true, env = "KOMUNIKATOR_HOME", value_name = "KATALOG")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Tworzy tożsamość tego urządzenia
    Init,
    /// Wypisuje Twój identyfikator do przekazania znajomym
    Id,
    /// Dodaje kontakt
    Add {
        /// Krótka nazwa kontaktu, jedno słowo
        alias: String,
        /// Identyfikator otrzymany od tej osoby
        id: EndpointId,
    },
    /// Usuwa kontakt
    Remove { alias: String },
    /// Wypisuje kontakty
    Contacts,
    /// Otwiera rozmowę z kontaktem
    Chat {
        alias: String,
        /// Nie używa żadnych zewnętrznych serwerów (sieć lokalna)
        #[arg(long)]
        local: bool,
        /// Łączy się z kontaktem pod znanym adresem IP:PORT
        #[arg(long, value_name = "IP:PORT")]
        direct: Option<SocketAddr>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Błąd: {err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let data_dir = match cli.data_dir {
        Some(dir) => dir,
        None => directories::ProjectDirs::from("", "", "komunikator")
            .context("nie mogę ustalić katalogu danych, podaj --data-dir")?
            .data_dir()
            .to_owned(),
    };
    let identity_path = identity::default_path(&data_dir);
    let contacts_path = contacts::default_path(&data_dir);

    match cli.command {
        Command::Init => {
            if Identity::exists(&identity_path) {
                bail!("tożsamość już istnieje: {}", identity_path.display());
            }
            let identity = Identity::create(&identity_path)?;
            println!("Utworzono tożsamość. Twój identyfikator:");
            println!("{}", identity.id());
        }
        Command::Id => println!("{}", load_identity(&identity_path)?.id()),
        Command::Add { alias, id } => {
            if id == load_identity(&identity_path)?.id() {
                bail!("to Twój własny identyfikator");
            }
            Contacts::load(&contacts_path)?.add(&alias, id)?;
            println!("Dodano kontakt `{alias}`.");
        }
        Command::Remove { alias } => {
            if !Contacts::load(&contacts_path)?.remove(&alias)? {
                bail!("nie znam kontaktu `{alias}`");
            }
            println!("Usunięto kontakt `{alias}`.");
        }
        Command::Contacts => {
            for (alias, id) in Contacts::load(&contacts_path)?.iter() {
                println!("{alias}\t{id}");
            }
        }
        Command::Chat {
            alias,
            local,
            direct,
        } => {
            let identity = load_identity(&identity_path)?;
            let contacts = Contacts::load(&contacts_path)?;
            chat(&identity, &contacts, &alias, local, direct).await?;
        }
    }
    Ok(())
}

fn load_identity(path: &Path) -> Result<Identity> {
    if !Identity::exists(path) {
        bail!("brak tożsamości, uruchom najpierw `szept init`");
    }
    Identity::load(path)
}

async fn chat(
    identity: &Identity,
    contacts: &Contacts,
    alias: &str,
    local: bool,
    direct: Option<SocketAddr>,
) -> Result<()> {
    let peer = contacts
        .get(alias)
        .with_context(|| format!("nie znam kontaktu `{alias}`"))?;
    let network = if local {
        Network::LocalOnly
    } else {
        Network::Public
    };
    let known = contacts.iter().map(|(_, id)| id);
    let (node, mut events) = Node::spawn(identity, known, network).await?;

    if local {
        println!("Tryb lokalny. Twoje adresy dla opcji --direct:");
        for addr in node.addr().ip_addrs() {
            println!("  {addr}");
        }
    } else if !node.wait_online(ONLINE_LIMIT).await {
        println!("Brak połączenia z siecią publiczną, próbuję dalej w tle.");
    }

    // Without a lookup service a contact can be dialled only at a known address.
    let target = match direct {
        Some(addr) => Some(EndpointAddr::new(peer).with_ip_addr(addr)),
        None if local => None,
        None => Some(EndpointAddr::from(peer)),
    };
    match target {
        Some(_) => println!("Łączę z `{alias}`…"),
        None => println!("Czekam, aż `{alias}` połączy się ze mną…"),
    }
    println!("Pisz i zatwierdzaj Enterem. Wyjście: /quit lub Ctrl+C.");

    let mut lines = spawn_stdin_reader();
    let mut pending: VecDeque<Outgoing> = VecDeque::new();
    let mut dialling: Option<JoinHandle<()>> = None;
    let mut retry = tokio::time::interval(RETRY_EVERY);
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            line = lines.recv() => {
                let Some(line) = line else { break };
                let line = line.trim();
                if line == "/quit" {
                    break;
                }
                if line.is_empty() {
                    continue;
                }
                match Outgoing::new(line) {
                    Ok(message) => pending.push_back(message),
                    Err(err) => {
                        println!("  ! {err}");
                        continue;
                    }
                }
                if node.is_connected(&peer) {
                    flush(&node, peer, &mut pending).await;
                } else {
                    println!("  … `{alias}` jest offline, wiadomość czeka w kolejce");
                }
            }
            event = events.recv() => {
                let Some(event) = event else { break };
                match event {
                    Event::Connected { peer: who } => {
                        println!("  * {} jest online", name(contacts, &who));
                        if who == peer {
                            flush(&node, peer, &mut pending).await;
                        }
                    }
                    Event::Disconnected { peer: who } => {
                        println!("  * {} jest offline", name(contacts, &who));
                    }
                    Event::Message { peer: who, body, .. } => {
                        println!("[{}] {}", name(contacts, &who), printable(&body));
                    }
                    Event::Delivered { .. } => println!("  ✓ dostarczono"),
                }
            }
            _ = retry.tick() => {
                let idle = dialling.as_ref().is_none_or(JoinHandle::is_finished);
                if let Some(target) = &target && idle && !node.is_connected(&peer) {
                    let (node, target) = (node.clone(), target.clone());
                    dialling = Some(tokio::spawn(async move {
                        // Failure means the contact is offline. The next tick retries.
                        let _ = node.connect(target).await;
                    }));
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    if !pending.is_empty() {
        println!(
            "Niewysłane wiadomości: {}. Kolejka nie jest jeszcze zapisywana na dysku.",
            pending.len()
        );
    }
    node.shutdown().await;
    Ok(())
}

/// Sends queued messages in order and stops at the first failure.
async fn flush(node: &Node, peer: EndpointId, pending: &mut VecDeque<Outgoing>) {
    while let Some(message) = pending.front() {
        if node.send(peer, message).await.is_err() {
            println!("  … połączenie zerwane, wiadomość czeka w kolejce");
            return;
        }
        pending.pop_front();
    }
}

fn name(contacts: &Contacts, id: &EndpointId) -> String {
    match contacts.alias_of(id) {
        Some(alias) => alias.to_owned(),
        None => id.fmt_short().to_string(),
    }
}

/// Replaces control characters, so a message cannot send escape sequences to
/// the terminal.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}

/// Reads standard input on its own thread, because terminal reads block.
fn spawn_stdin_reader() -> mpsc::Receiver<String> {
    let (lines_tx, lines_rx) = mpsc::channel(16);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if lines_tx.blocking_send(line).is_err() {
                break;
            }
        }
    });
    lines_rx
}

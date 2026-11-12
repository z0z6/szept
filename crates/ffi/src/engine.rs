//! The background task behind a running [`crate::App`]: keeps trying to reach
//! offline contacts and delivers queued messages once they are back.
//!
//! The queue lives in memory. Milestone 2 moves it into the encrypted
//! database inside `komunikator-core`.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};

use komunikator_core::{EndpointAddr, EndpointId, Event, Node, Outgoing, protocol::MAX_MEDIA_SIZE};
use tokio::{
    sync::mpsc::{Receiver, UnboundedReceiver},
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};

use crate::{ChatEvent, EventListener, Shared, download_task};

/// How often offline contacts are dialled again.
const RETRY_EVERY: Duration = Duration::from_secs(10);

pub(crate) enum Command {
    Send {
        peer: EndpointId,
        message: Outgoing,
    },
    ContactAdded(EndpointId),
    ContactRemoved(EndpointId),
    Dial(EndpointAddr),
    /// The device switched networks: look around and redial everyone now.
    NetworkChanged,
    /// Our profile picture changed: send it to everyone connected.
    AvatarChanged,
}

pub(crate) async fn run(
    node: Node,
    mut events: Receiver<Event>,
    mut commands: UnboundedReceiver<Command>,
    listener: Arc<dyn EventListener>,
    shared: Arc<Shared>,
    contacts: Vec<EndpointId>,
    dial_by_id: bool,
) {
    let mut contacts: HashSet<EndpointId> = contacts.into_iter().collect();
    let mut outbox: HashMap<EndpointId, VecDeque<Outgoing>> = HashMap::new();
    let mut dialling: HashMap<EndpointId, JoinHandle<()>> = HashMap::new();
    let mut retry = interval(RETRY_EVERY);
    retry.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    Command::Send { peer, message } => {
                        outbox.entry(peer).or_default().push_back(message);
                        if node.is_connected(&peer) {
                            flush(&node, peer, &mut outbox, &*listener).await;
                        }
                    }
                    Command::ContactAdded(peer) => {
                        contacts.insert(peer);
                    }
                    Command::ContactRemoved(peer) => {
                        contacts.remove(&peer);
                        outbox.remove(&peer);
                    }
                    Command::Dial(addr) => dial(&node, addr, &mut dialling),
                    Command::NetworkChanged => {
                        node.network_changed().await;
                        // Old attempts may be stuck on the network that is gone.
                        for (_, attempt) in dialling.drain() {
                            attempt.abort();
                        }
                        if dial_by_id {
                            for peer in &contacts {
                                dial(&node, EndpointAddr::from(*peer), &mut dialling);
                            }
                        }
                    }
                    Command::AvatarChanged => {
                        for peer in contacts.iter().filter(|peer| node.is_connected(peer)) {
                            send_avatar(&node, *peer, &shared);
                        }
                    }
                }
            }
            event = events.recv() => {
                let Some(event) = event else { break };
                let mut connected = None;
                let mut fetch = None;
                match &event {
                    Event::Connected { peer } => connected = Some(*peer),
                    Event::FileOffered { peer, id, offer, media, .. } => {
                        shared
                            .offers
                            .lock()
                            .unwrap()
                            .insert(id.to_string(), (*peer, *id, offer.clone()));
                        // Voice messages and GIFs are fetched without asking.
                        if media.is_some() && offer.size <= MAX_MEDIA_SIZE {
                            fetch = Some(id.to_string());
                        }
                    }
                    // Only a real change is worth redrawing for.
                    Event::Avatar { peer, image } if !shared.store_avatar(peer, image) => continue,
                    _ => {}
                }
                // Report the connection before the queued messages go out,
                // so the interface sees "connected" ahead of "sent".
                listener.on_event(event.into());
                if let Some(id) = fetch {
                    let dir = shared.media_dir.clone();
                    if let Ok(Some(task)) =
                        download_task(&shared, node.clone(), listener.clone(), id, dir)
                    {
                        tokio::spawn(task);
                    }
                }
                if let Some(peer) = connected {
                    send_avatar(&node, peer, &shared);
                    flush(&node, peer, &mut outbox, &*listener).await;
                }
            }
            _ = retry.tick() => {
                if dial_by_id {
                    for peer in &contacts {
                        dial(&node, EndpointAddr::from(*peer), &mut dialling);
                    }
                }
            }
        }
    }
}

/// Sends our profile picture to a connected contact, in the background.
/// Sent even when empty, so a contact learns that we removed it.
fn send_avatar(node: &Node, peer: EndpointId, shared: &Shared) {
    let image = shared.avatar.lock().unwrap().clone();
    let node = node.clone();
    tokio::spawn(async move {
        // Failure means the connection just dropped; the next one retries.
        let _ = node.send_avatar(peer, &image).await;
    });
}

/// Starts connecting to a contact, unless connected or already trying.
fn dial(node: &Node, addr: EndpointAddr, dialling: &mut HashMap<EndpointId, JoinHandle<()>>) {
    let peer = addr.id;
    let busy = dialling
        .get(&peer)
        .is_some_and(|attempt| !attempt.is_finished());
    if busy || node.is_connected(&peer) {
        return;
    }
    let node = node.clone();
    let attempt = tokio::spawn(async move {
        // Failure means the contact is offline. The next tick retries.
        let _ = node.connect(addr).await;
    });
    dialling.insert(peer, attempt);
}

/// Sends the queued messages for `peer` in order, stopping at the first
/// failure. What is left goes out after the next reconnect. Every message
/// that leaves is reported as [`ChatEvent::Sent`].
async fn flush(
    node: &Node,
    peer: EndpointId,
    outbox: &mut HashMap<EndpointId, VecDeque<Outgoing>>,
    listener: &dyn EventListener,
) {
    let Some(queue) = outbox.get_mut(&peer) else {
        return;
    };
    while let Some(message) = queue.front() {
        if node.send(peer, message).await.is_err() {
            return;
        }
        listener.on_event(ChatEvent::Sent {
            peer: peer.to_string(),
            id: message.id.to_string(),
        });
        queue.pop_front();
    }
}

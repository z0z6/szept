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

use komunikator_core::{EndpointAddr, EndpointId, Event, Node, Outgoing};
use tokio::{
    sync::mpsc::{Receiver, UnboundedReceiver},
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};

use crate::{ChatEvent, EventListener};

/// How often offline contacts are dialled again.
const RETRY_EVERY: Duration = Duration::from_secs(10);

pub(crate) enum Command {
    Send { peer: EndpointId, message: Outgoing },
    ContactAdded(EndpointId),
    ContactRemoved(EndpointId),
    Dial(EndpointAddr),
}

pub(crate) async fn run(
    node: Node,
    mut events: Receiver<Event>,
    mut commands: UnboundedReceiver<Command>,
    listener: Arc<dyn EventListener>,
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
                }
            }
            event = events.recv() => {
                let Some(event) = event else { break };
                let connected = match &event {
                    Event::Connected { peer } => Some(*peer),
                    _ => None,
                };
                // Report the connection before the queued messages go out,
                // so the interface sees "connected" ahead of "sent".
                listener.on_event(event.into());
                if let Some(peer) = connected {
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

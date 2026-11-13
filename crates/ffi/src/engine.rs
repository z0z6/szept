//! The background task behind a running [`crate::App`]: keeps trying to reach
//! people with something queued for them, delivers the queue once they are
//! back, and turns what arrives into [`ChatEvent`]s.
//!
//! The queue lives in memory. Milestone 2 moves it into the encrypted
//! database inside `komunikator-core`.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use komunikator_core::{
    EndpointAddr, EndpointId, Event, GroupId, GroupInfo, MessageId, Node, Outgoing, Post, Signal,
    files, protocol::MAX_MEDIA_SIZE,
};
use tokio::{
    sync::mpsc::{Receiver, UnboundedReceiver},
    task::JoinHandle,
    time::{MissedTickBehavior, interval},
};

use crate::{ChatEvent, EventListener, Media, Shared, download_task};

/// How often people with something queued, and contacts, are dialled again.
const RETRY_EVERY: Duration = Duration::from_secs(10);

/// How many group message ids are remembered to label their receipts.
const REMEMBERED_GROUP_MESSAGES: usize = 4096;

/// Something queued for one person.
pub(crate) enum Item {
    Message(Outgoing),
    Signal(Signal),
}

impl Clone for Item {
    fn clone(&self) -> Self {
        match self {
            Self::Message(message) => Self::Message(message.clone()),
            Self::Signal(signal) => Self::Signal(signal.clone()),
        }
    }
}

pub(crate) enum Command {
    /// Sends `item` to each of `peers`. `group` labels a group message.
    Queue {
        peers: Vec<EndpointId>,
        item: Item,
        group: Option<GroupId>,
    },
    ContactRemoved(EndpointId),
    Dial(EndpointAddr),
    /// The device switched networks: look around and redial everyone now.
    NetworkChanged,
    /// Our picture or name changed: tell everyone connected.
    ProfileChanged,
}

struct Engine {
    node: Node,
    listener: Arc<dyn EventListener>,
    shared: Arc<Shared>,
    dial_by_id: bool,
    outbox: HashMap<EndpointId, VecDeque<Item>>,
    dialling: HashMap<EndpointId, JoinHandle<()>>,
    /// Which group each recent group message of ours belongs to.
    group_of: HashMap<MessageId, GroupId>,
    group_order: VecDeque<MessageId>,
}

pub(crate) async fn run(
    node: Node,
    mut events: Receiver<Event>,
    mut commands: UnboundedReceiver<Command>,
    listener: Arc<dyn EventListener>,
    shared: Arc<Shared>,
    dial_by_id: bool,
) {
    let mut engine = Engine {
        node,
        listener,
        shared,
        dial_by_id,
        outbox: HashMap::new(),
        dialling: HashMap::new(),
        group_of: HashMap::new(),
        group_order: VecDeque::new(),
    };
    let mut retry = interval(RETRY_EVERY);
    retry.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                engine.command(command).await;
            }
            event = events.recv() => {
                let Some(event) = event else { break };
                engine.event(event).await;
            }
            _ = retry.tick() => engine.redial(),
        }
    }
}

impl Engine {
    async fn command(&mut self, command: Command) {
        match command {
            Command::Queue { peers, item, group } => {
                if let (Some(group), Item::Message(message)) = (group, &item) {
                    self.remember(message.id, group);
                }
                for peer in peers {
                    self.outbox.entry(peer).or_default().push_back(item.clone());
                    if self.node.is_connected(&peer) {
                        self.flush(peer).await;
                    } else if self.dial_by_id {
                        self.dial(EndpointAddr::from(peer));
                    }
                }
            }
            Command::ContactRemoved(peer) => {
                if !self.shared.groups.lock().unwrap().knows(&peer) {
                    self.outbox.remove(&peer);
                }
            }
            Command::Dial(addr) => self.dial(addr),
            Command::NetworkChanged => {
                self.node.network_changed().await;
                // Old attempts may be stuck on the network that is gone.
                for (_, attempt) in self.dialling.drain() {
                    attempt.abort();
                }
                self.redial();
            }
            Command::ProfileChanged => {
                let everyone: Vec<EndpointId> = self
                    .shared
                    .groups
                    .lock()
                    .unwrap()
                    .all_members(&self.shared.me);
                let contacts: Vec<EndpointId> = self
                    .shared
                    .contacts
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(_, id)| id)
                    .collect();
                let mut peers = contacts;
                peers.extend(everyone);
                peers.sort_by_key(|id| *id.as_bytes());
                peers.dedup();
                for peer in peers
                    .into_iter()
                    .filter(|peer| self.node.is_connected(peer))
                {
                    self.send_profile(peer);
                }
            }
        }
    }

    async fn event(&mut self, event: Event) {
        let me = self.shared.me;
        match event {
            Event::Connected { peer } => {
                // Report the connection before the queued messages go out,
                // so the interface sees "connected" ahead of "sent".
                self.emit(ChatEvent::Connected {
                    peer: peer.to_string(),
                });
                self.send_profile(peer);
                // Group invitations sent while they were away go out now.
                let owned: Vec<GroupInfo> = self
                    .shared
                    .groups
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|info| info.owner == *me.as_bytes() && info.has(&peer))
                    .cloned()
                    .collect();
                for info in owned {
                    self.outbox
                        .entry(peer)
                        .or_default()
                        .push_front(Item::Signal(Signal::Group(info)));
                }
                self.flush(peer).await;
            }
            Event::Disconnected { peer } => self.emit(ChatEvent::Disconnected {
                peer: peer.to_string(),
            }),
            Event::Message {
                peer,
                id,
                sent_at_ms,
                body,
            } => {
                // Group members who are not contacts may only talk in groups.
                if self.shared.is_contact(&peer) {
                    self.emit(ChatEvent::Message {
                        peer: peer.to_string(),
                        id: id.to_string(),
                        sent_at_ms,
                        body,
                        group: None,
                    });
                }
            }
            Event::Delivered { peer, id } => self.emit(ChatEvent::Delivered {
                peer: peer.to_string(),
                id: id.to_string(),
                group: self.group_of.get(&id).map(ToString::to_string),
            }),
            Event::FileOffered {
                peer,
                id,
                sent_at_ms,
                offer,
                media,
            } => {
                if self.shared.is_contact(&peer) {
                    self.offered(peer, id, sent_at_ms, offer, media, None);
                }
            }
            Event::Avatar { peer, image } => {
                // Only a real change is worth redrawing for.
                if self.shared.store_avatar(&peer, &image) {
                    self.emit(ChatEvent::AvatarChanged {
                        peer: peer.to_string(),
                    });
                }
            }
            Event::Post {
                peer,
                id,
                sent_at_ms,
                group,
                post,
            } => {
                if !self.shared.may_post(&peer, group.as_ref()) {
                    return;
                }
                let group_text = group.map(|group| group.to_string());
                match post {
                    Post::Text(body) => self.emit(ChatEvent::Message {
                        peer: peer.to_string(),
                        id: id.to_string(),
                        sent_at_ms,
                        body,
                        group: group_text,
                    }),
                    Post::Sticker(code) => self.emit(ChatEvent::Sticker {
                        peer: peer.to_string(),
                        id: id.to_string(),
                        sent_at_ms,
                        code,
                        group: group_text,
                    }),
                    Post::File { offer, media } => {
                        self.offered(peer, id, sent_at_ms, offer, media, group);
                    }
                }
            }
            Event::Reaction {
                peer,
                target,
                group,
                reaction,
            } => {
                if self.shared.may_post(&peer, group.as_ref()) {
                    self.emit(ChatEvent::Reaction {
                        peer: peer.to_string(),
                        target: target.to_string(),
                        reaction,
                        group: group.map(|group| group.to_string()),
                    });
                }
            }
            Event::Group { peer, info } => self.group_update(peer, info),
            Event::Left { peer, group } => self.left(peer, group),
            Event::Name { peer, name } => {
                if self.shared.store_name(&peer, &name) {
                    self.emit(ChatEvent::NameChanged {
                        peer: peer.to_string(),
                    });
                }
            }
        }
    }

    /// Registers a file offer, reports it and fetches media at once.
    fn offered(
        &mut self,
        peer: EndpointId,
        id: MessageId,
        sent_at_ms: u64,
        offer: komunikator_core::FileOffer,
        media: Option<komunikator_core::Media>,
        group: Option<GroupId>,
    ) {
        self.shared
            .offers
            .lock()
            .unwrap()
            .insert(id.to_string(), (peer, id, offer.clone()));
        self.emit(ChatEvent::FileOffered {
            peer: peer.to_string(),
            id: id.to_string(),
            sent_at_ms,
            hash: files::hash_hex(&offer.hash),
            name: offer.name.clone(),
            size: offer.size,
            media: media.map(Media::from),
            group: group.map(|group| group.to_string()),
        });
        // Voice messages and GIFs are fetched without asking.
        if media.is_some() && offer.size <= MAX_MEDIA_SIZE {
            let dir = self.shared.media_dir.clone();
            if let Ok(Some(task)) = download_task(
                &self.shared,
                self.node.clone(),
                self.listener.clone(),
                id.to_string(),
                dir,
            ) {
                tokio::spawn(task);
            }
        }
    }

    /// A group's state arrived. Only its owner may change it.
    fn group_update(&mut self, peer: EndpointId, info: GroupInfo) {
        let me = self.shared.me;
        if info.owner() != Some(peer) {
            return;
        }
        let known = self.shared.groups.lock().unwrap().get(&info.id).cloned();
        if known
            .as_ref()
            .is_some_and(|known| known.owner != info.owner)
        {
            return;
        }
        if !info.has(&me) {
            // Taken out of the group.
            if known.is_some_and(|known| known.version < info.version) {
                let _ = self.shared.groups.lock().unwrap().remove(&info.id);
                self.shared.sync_allowed(&self.node);
                self.emit(ChatEvent::GroupClosed {
                    group: info.id.to_string(),
                    reason: "removed".into(),
                });
            }
            return;
        }
        let id = info.id;
        let accepted = self
            .shared
            .groups
            .lock()
            .unwrap()
            .accept(info)
            .unwrap_or(false);
        if accepted {
            self.shared.sync_allowed(&self.node);
            self.emit(ChatEvent::GroupChanged {
                group: id.to_string(),
            });
        }
    }

    /// Someone left a group. When it is the owner, the group is closed;
    /// when we own it, we take them off the list and tell the others.
    fn left(&mut self, peer: EndpointId, group: GroupId) {
        let me = self.shared.me;
        let Some(known) = self.shared.groups.lock().unwrap().get(&group).cloned() else {
            return;
        };
        if !known.has(&peer) {
            return;
        }
        if known.owner == *peer.as_bytes() {
            let _ = self.shared.groups.lock().unwrap().remove(&group);
            self.shared.sync_allowed(&self.node);
            self.emit(ChatEvent::GroupClosed {
                group: group.to_string(),
                reason: "closed".into(),
            });
        } else if known.owner == *me.as_bytes() {
            let mut info = known;
            info.members.retain(|member| member.id != *peer.as_bytes());
            info.version += 1;
            if self
                .shared
                .groups
                .lock()
                .unwrap()
                .accept(info.clone())
                .unwrap_or(false)
            {
                self.shared.sync_allowed(&self.node);
                for other in info.others(&me) {
                    self.outbox
                        .entry(other)
                        .or_default()
                        .push_back(Item::Signal(Signal::Group(info.clone())));
                    if self.dial_by_id {
                        self.dial(EndpointAddr::from(other));
                    }
                }
                self.emit(ChatEvent::GroupChanged {
                    group: group.to_string(),
                });
            }
        }
    }

    /// Sends our picture (even when empty, so a removal reaches them) and
    /// our name, in the background.
    fn send_profile(&self, peer: EndpointId) {
        let image = self.shared.avatar.lock().unwrap().clone();
        let name = self.shared.name_signal();
        let node = self.node.clone();
        tokio::spawn(async move {
            // Failure means the connection just dropped; the next one retries.
            let _ = node.send_avatar(peer, &image).await;
            if let Some(name) = name {
                let _ = node.signal(peer, &name).await;
            }
        });
    }

    /// Dials contacts and everyone with something queued.
    fn redial(&mut self) {
        if !self.dial_by_id {
            return;
        }
        let mut peers: Vec<EndpointId> = self
            .shared
            .contacts
            .lock()
            .unwrap()
            .iter()
            .map(|(_, id)| id)
            .collect();
        peers.extend(
            self.outbox
                .iter()
                .filter(|(_, queue)| !queue.is_empty())
                .map(|(peer, _)| *peer),
        );
        for peer in peers {
            self.dial(EndpointAddr::from(peer));
        }
    }

    /// Starts connecting, unless connected or already trying.
    fn dial(&mut self, addr: EndpointAddr) {
        let peer = addr.id;
        let busy = self
            .dialling
            .get(&peer)
            .is_some_and(|attempt| !attempt.is_finished());
        if busy || self.node.is_connected(&peer) {
            return;
        }
        let node = self.node.clone();
        let attempt = tokio::spawn(async move {
            // Failure means they are offline. The next tick retries.
            let _ = node.connect(addr).await;
        });
        self.dialling.insert(peer, attempt);
    }

    /// Sends what is queued for `peer` in order, stopping at the first
    /// failure. What is left goes out after the next reconnect. Every message
    /// that leaves is reported as [`ChatEvent::Sent`].
    async fn flush(&mut self, peer: EndpointId) {
        let Some(queue) = self.outbox.get_mut(&peer) else {
            return;
        };
        while let Some(item) = queue.front() {
            match item {
                Item::Message(message) => {
                    if self.node.send(peer, message).await.is_err() {
                        return;
                    }
                    self.listener.on_event(ChatEvent::Sent {
                        peer: peer.to_string(),
                        id: message.id.to_string(),
                        group: self.group_of.get(&message.id).map(ToString::to_string),
                    });
                }
                Item::Signal(signal) => {
                    if self.node.signal(peer, signal).await.is_err() {
                        return;
                    }
                }
            }
            queue.pop_front();
        }
    }

    fn remember(&mut self, id: MessageId, group: GroupId) {
        self.group_of.insert(id, group);
        self.group_order.push_back(id);
        while self.group_order.len() > REMEMBERED_GROUP_MESSAGES {
            if let Some(old) = self.group_order.pop_front() {
                self.group_of.remove(&old);
            }
        }
    }

    fn emit(&self, event: ChatEvent) {
        self.listener.on_event(event);
    }
}

//! Conversations kept in memory while the app runs, as on Android.
//! Persistent, encrypted history comes with milestone 2.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use komunikator_ffi::{ArchivedConversation, ArchivedMessage, ArchivedReaction, Media};
use serde::Serialize;

/// Numbers the app's own notes, so two in one millisecond differ.
static NOTES: AtomicU64 = AtomicU64::new(0);

/// Where a message is on its way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Ours, waiting for the contact to be reachable.
    Queued,
    /// Ours, left this device.
    Sent,
    /// Ours, confirmed by the contact's device.
    Delivered,
    /// Theirs.
    Received,
}

impl Status {
    fn rank(self) -> u8 {
        match self {
            Self::Queued => 0,
            Self::Sent => 1,
            Self::Delivered | Self::Received => 2,
        }
    }
}

/// Where a file offered to us stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transfer {
    /// Ours: offered, the contact fetches it from this device.
    Shared,
    /// Theirs, not fetched yet.
    Offered,
    Downloading,
    Saved,
    Failed,
    /// Restored from an archive without its content.
    Archived,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub hash: String,
    /// "program", "archive", "image", "audio", "video", "document", "other".
    pub kind: String,
    pub transfer: Transfer,
    pub done: u64,
    /// Where the file is on this device: ours from the start, theirs once
    /// saved.
    pub path: Option<String>,
    pub reason: Option<String>,
    /// "voice" or "gif" for media the app fetches on its own, else `None`.
    pub media: Option<&'static str>,
    /// Length of a voice message.
    pub duration_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: String,
    pub mine: bool,
    /// Who wrote it, in a group: their identifier.
    pub author: Option<String>,
    pub body: String,
    pub at_ms: u64,
    pub status: Status,
    pub file: Option<FileInfo>,
    /// The code of an animated sticker.
    pub sticker: Option<String>,
    /// A note from the app itself, such as a key change.
    pub system: bool,
    /// One reaction per person: identifier → reaction code.
    pub reactions: BTreeMap<String, String>,
    /// For our group messages: how many members must confirm, and who did.
    pub recipients: u32,
    #[serde(skip)]
    pub delivered_by: Vec<String>,
}

impl Message {
    pub fn outgoing(id: String, body: String) -> Self {
        Self {
            id,
            mine: true,
            author: None,
            body,
            at_ms: now_ms(),
            status: Status::Queued,
            file: None,
            sticker: None,
            system: false,
            reactions: BTreeMap::new(),
            recipients: 1,
            delivered_by: Vec::new(),
        }
    }

    pub fn incoming(id: String, author: Option<String>, at_ms: u64) -> Self {
        Self {
            mine: false,
            author,
            at_ms,
            status: Status::Received,
            ..Self::outgoing(id, String::new())
        }
    }

    /// A note from the app, such as "the contact changed their key".
    pub fn system(text: String) -> Self {
        Self {
            id: format!(
                "system-{}-{}",
                now_ms(),
                NOTES.fetch_add(1, Ordering::Relaxed)
            ),
            mine: false,
            system: true,
            status: Status::Received,
            ..Self::outgoing(String::new(), text)
        }
    }

    pub fn outgoing_file(id: String, file: FileInfo) -> Self {
        Self {
            file: Some(file),
            ..Self::outgoing(id, String::new())
        }
    }
}

#[derive(Debug, Default)]
pub struct Store {
    conversations: HashMap<String, Vec<Message>>,
    unread: HashMap<String, u32>,
    /// Statuses that arrived before their message was stored.
    early: HashMap<String, Status>,
}

impl Store {
    pub fn push(&mut self, peer: &str, mut message: Message) {
        if let Some(status) = self.early.remove(&message.id)
            && status.rank() > message.status.rank()
        {
            message.status = status;
        }
        let conversation = self.conversations.entry(peer.to_owned()).or_default();
        if !conversation.iter().any(|old| old.id == message.id) {
            conversation.push(message);
        }
    }

    /// Stores an incoming message or file offer. Returns `false` for a
    /// repeat of one already shown, which happens when the sender retries.
    pub fn receive(&mut self, peer: &str, message: Message, seen: bool) -> bool {
        let conversation = self.conversations.entry(peer.to_owned()).or_default();
        if conversation.iter().any(|old| old.id == message.id) {
            return false;
        }
        conversation.push(message);
        if !seen {
            *self.unread.entry(peer.to_owned()).or_default() += 1;
        }
        true
    }

    /// Records that `who` confirmed our group message; it counts as
    /// delivered once every recipient did. Returns `true` on a change.
    pub fn delivered_by(&mut self, conversation: &str, id: &str, who: &str) -> bool {
        let Some(message) = self.find(conversation, id) else {
            return false;
        };
        if !message.delivered_by.iter().any(|known| known == who) {
            message.delivered_by.push(who.to_owned());
        }
        if message.delivered_by.len() as u32 >= message.recipients {
            let changed = message.status != Status::Delivered;
            message.status = Status::Delivered;
            return changed;
        }
        false
    }

    /// Sets or, with an empty `reaction`, removes the reaction of `who`.
    pub fn react(&mut self, conversation: &str, target: &str, who: &str, reaction: &str) -> bool {
        let Some(message) = self.find(conversation, target) else {
            return false;
        };
        let old = if reaction.is_empty() {
            message.reactions.remove(who)
        } else {
            message
                .reactions
                .insert(who.to_owned(), reaction.to_owned())
        };
        old.as_deref() != Some(reaction)
    }

    /// Adds a note to a conversation.
    pub fn note(&mut self, conversation: &str, text: String) {
        self.conversations
            .entry(conversation.to_owned())
            .or_default()
            .push(Message::system(text));
    }

    /// Moves a conversation to a contact's new key: the old one ends with
    /// a note and the new one carries on below it.
    pub fn migrate(&mut self, old: &str, new: &str, note: String) {
        let mut moved = self.conversations.remove(old).unwrap_or_default();
        moved.push(Message::system(note));
        let existing = self.conversations.remove(new).unwrap_or_default();
        moved.extend(existing);
        self.conversations.insert(new.to_owned(), moved);
        if let Some(count) = self.unread.remove(old) {
            *self.unread.entry(new.to_owned()).or_default() += count;
        }
    }

    /// Everything, in the archive's form. `me` is our identifier.
    pub fn to_archive(&self, me: &str) -> Vec<ArchivedConversation> {
        self.conversations
            .iter()
            .map(|(key, messages)| ArchivedConversation {
                key: key.clone(),
                messages: messages
                    .iter()
                    .map(|message| archived(message, me))
                    .collect(),
            })
            .collect()
    }

    /// Adds conversations from an archive, skipping messages already here.
    pub fn merge_archive(&mut self, conversations: Vec<ArchivedConversation>, me: &str) {
        for conversation in conversations {
            let target = self.conversations.entry(conversation.key).or_default();
            for message in conversation.messages {
                if target.iter().any(|known| known.id == message.id) {
                    continue;
                }
                target.push(restored(message, me));
            }
            target.sort_by_key(|message| message.at_ms);
        }
    }

    /// Moves our message forward, never back: the confirmation may in theory
    /// overtake the report that the message left. Returns `true` on a change.
    pub fn set_status(&mut self, peer: &str, id: &str, status: Status) -> bool {
        let Some(message) = self.find(peer, id) else {
            // The core can report on a message before the command that sent
            // it has stored it. Keep the news until then.
            let early = self.early.entry(id.to_owned()).or_insert(status);
            if status.rank() > early.rank() {
                *early = status;
            }
            return false;
        };
        if status.rank() <= message.status.rank() {
            return false;
        }
        message.status = status;
        true
    }

    /// Updates a file offered to us. Returns `true` on a change.
    pub fn set_transfer(
        &mut self,
        peer: &str,
        id: &str,
        update: impl FnOnce(&mut FileInfo),
    ) -> bool {
        match self
            .find(peer, id)
            .and_then(|message| message.file.as_mut())
        {
            Some(file) => {
                update(file);
                true
            }
            None => false,
        }
    }

    /// Updates a file wherever its message is: file events name the sender,
    /// and a group file sits in the group's conversation. Returns the
    /// conversation it was in.
    pub fn update_file(
        &mut self,
        peer: &str,
        id: &str,
        update: impl FnOnce(&mut FileInfo),
    ) -> Option<String> {
        let key = if self.find(peer, id).is_some() {
            peer.to_owned()
        } else {
            self.conversations
                .iter()
                .find(|(_, messages)| messages.iter().any(|message| message.id == id))
                .map(|(key, _)| key.clone())?
        };
        let file = self.find(&key, id)?.file.as_mut()?;
        update(file);
        Some(key)
    }

    pub fn file(&self, peer: &str, id: &str) -> Option<FileInfo> {
        self.conversations
            .get(peer)?
            .iter()
            .find(|message| message.id == id)?
            .file
            .clone()
    }

    pub fn conversation(&self, peer: &str) -> Vec<Message> {
        self.conversations.get(peer).cloned().unwrap_or_default()
    }

    pub fn last_body(&self, peer: &str) -> Option<String> {
        let message = self.conversations.get(peer)?.last()?;
        let text = match &message.file {
            _ if message.sticker.is_some() => "Naklejka".to_owned(),
            Some(file) if file.media == Some("voice") => "🎤 Wiadomość głosowa".to_owned(),
            Some(file) if file.media == Some("gif") => "GIF".to_owned(),
            Some(file) => format!("📎 {}", file.name),
            None => message.body.clone(),
        };
        let mut preview: String = text.chars().take(60).collect();
        if preview.len() < text.len() {
            preview.push('…');
        }
        Some(if message.mine {
            format!("Ty: {preview}")
        } else {
            preview
        })
    }

    pub fn unread(&self, peer: &str) -> u32 {
        self.unread.get(peer).copied().unwrap_or(0)
    }

    /// Returns `true` if there was anything unread.
    pub fn mark_read(&mut self, peer: &str) -> bool {
        self.unread.remove(peer).is_some_and(|count| count > 0)
    }

    pub fn forget(&mut self, peer: &str) {
        self.conversations.remove(peer);
        self.unread.remove(peer);
    }

    fn find(&mut self, peer: &str, id: &str) -> Option<&mut Message> {
        self.conversations
            .get_mut(peer)?
            .iter_mut()
            .find(|message| message.id == id)
    }
}

fn archived(message: &Message, me: &str) -> ArchivedMessage {
    let file = message.file.as_ref();
    ArchivedMessage {
        id: message.id.clone(),
        author: if message.mine {
            String::new()
        } else {
            message.author.clone().unwrap_or_default()
        },
        at_ms: message.at_ms,
        kind: if message.system {
            "system"
        } else if message.sticker.is_some() {
            "sticker"
        } else if file.is_some() {
            "file"
        } else {
            "text"
        }
        .into(),
        body: message
            .sticker
            .clone()
            .unwrap_or_else(|| message.body.clone()),
        file_name: file.map(|file| file.name.clone()),
        file_size: file.map_or(0, |file| file.size),
        file_hash: file.map(|file| file.hash.clone()),
        media: file.and_then(|file| match file.media {
            Some("voice") => Some(Media::Voice {
                duration_ms: file.duration_ms.unwrap_or(0),
            }),
            Some("gif") => Some(Media::Gif),
            _ => None,
        }),
        file_path: file.and_then(|file| file.path.clone()),
        status: match message.status {
            Status::Queued => "queued",
            Status::Sent => "sent",
            Status::Delivered => "delivered",
            Status::Received => "received",
        }
        .into(),
        reactions: message
            .reactions
            .iter()
            .map(|(who, reaction)| ArchivedReaction {
                who: if who == me {
                    String::new()
                } else {
                    who.clone()
                },
                reaction: reaction.clone(),
            })
            .collect(),
    }
}

fn restored(message: ArchivedMessage, me: &str) -> Message {
    let mine = message.author.is_empty() && message.kind != "system";
    let file = message.file_name.map(|name| {
        let (media, duration_ms) = match message.media {
            Some(Media::Voice { duration_ms }) => (Some("voice"), Some(duration_ms)),
            Some(Media::Gif) => (Some("gif"), None),
            None => (None, None),
        };
        let present = message
            .file_path
            .as_deref()
            .is_some_and(|path| Path::new(path).exists());
        FileInfo {
            kind: komunikator_ffi::file_kind(name.clone()),
            name,
            size: message.file_size,
            hash: message.file_hash.unwrap_or_default(),
            transfer: if present {
                Transfer::Saved
            } else {
                Transfer::Archived
            },
            done: if present { message.file_size } else { 0 },
            path: if present { message.file_path } else { None },
            reason: None,
            media,
            duration_ms,
        }
    });
    Message {
        id: message.id,
        mine,
        author: (!mine && !message.author.is_empty()).then_some(message.author),
        body: if message.kind == "sticker" {
            String::new()
        } else {
            message.body.clone()
        },
        at_ms: message.at_ms,
        status: match message.status.as_str() {
            "queued" | "sent" => Status::Sent,
            "delivered" => Status::Delivered,
            _ => Status::Received,
        },
        file,
        sticker: (message.kind == "sticker").then_some(message.body),
        system: message.kind == "system",
        reactions: message
            .reactions
            .into_iter()
            .map(|reaction| {
                let who = if reaction.who.is_empty() {
                    me.to_owned()
                } else {
                    reaction.who
                };
                (who, reaction.reaction)
            })
            .collect(),
        recipients: 1,
        delivered_by: Vec::new(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(id: &str, body: &str) -> Message {
        Message {
            body: body.into(),
            ..Message::incoming(id.into(), None, 0)
        }
    }

    #[test]
    fn reactions_key_changes_and_archives() {
        let mut store = Store::default();
        store.push("stary", Message::outgoing("1".into(), "hej".into()));
        assert!(store.react("stary", "1", "ola", "pierog"));
        assert!(!store.react("stary", "1", "ola", "pierog"));
        assert!(store.react("stary", "1", "ja", "kapec"));
        store.migrate("stary", "nowy", "Nowy klucz".into());
        let moved = store.conversation("nowy");
        assert_eq!(moved.len(), 2);
        assert!(moved[1].system);
        assert!(store.conversation("stary").is_empty());

        let archive = store.to_archive("ja");
        assert_eq!(archive[0].messages[0].reactions.len(), 2);
        let mut fresh = Store::default();
        fresh.merge_archive(archive.clone(), "ja");
        fresh.merge_archive(archive, "ja");
        let back = fresh.conversation("nowy");
        assert_eq!(back.len(), 2);
        assert!(back[0].mine);
        assert_eq!(
            back[0].reactions.get("ja").map(String::as_str),
            Some("kapec")
        );
    }

    #[test]
    fn group_messages_count_as_delivered_once_everyone_confirms() {
        let mut store = Store::default();
        let mut message = Message::outgoing("1".into(), "hej grupo".into());
        message.recipients = 2;
        store.push("grupa", message);
        assert!(!store.delivered_by("grupa", "1", "ola"));
        assert!(!store.delivered_by("grupa", "1", "ola"));
        assert!(store.delivered_by("grupa", "1", "ela"));
        assert_eq!(store.conversation("grupa")[0].status, Status::Delivered);
    }

    #[test]
    fn statuses_only_move_forward_and_repeats_are_dropped() {
        let mut store = Store::default();
        store.push("ola", Message::outgoing("1".into(), "hej".into()));
        assert!(store.set_status("ola", "1", Status::Delivered));
        assert!(!store.set_status("ola", "1", Status::Sent));
        assert_eq!(store.conversation("ola")[0].status, Status::Delivered);

        assert!(store.receive("ola", text("2", "cześć 👋"), false));
        assert!(!store.receive("ola", text("2", "cześć 👋"), false));
        assert_eq!(store.unread("ola"), 1);
        assert_eq!(store.last_body("ola").as_deref(), Some("cześć 👋"));
        assert!(store.mark_read("ola"));
        assert!(!store.mark_read("ola"));
    }

    #[test]
    fn a_status_that_arrives_first_is_kept() {
        let mut store = Store::default();
        assert!(!store.set_status("ola", "9", Status::Sent));
        store.push("ola", Message::outgoing("9".into(), "szybko".into()));
        assert_eq!(store.conversation("ola")[0].status, Status::Sent);
    }
}

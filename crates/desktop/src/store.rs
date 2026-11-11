//! Conversations kept in memory while the app runs, as on Android.
//! Persistent, encrypted history comes with milestone 2.

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

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
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: String,
    pub mine: bool,
    pub body: String,
    pub at_ms: u64,
    pub status: Status,
    pub file: Option<FileInfo>,
}

impl Message {
    pub fn outgoing(id: String, body: String) -> Self {
        Self {
            id,
            mine: true,
            body,
            at_ms: now_ms(),
            status: Status::Queued,
            file: None,
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
            id: id.into(),
            mine: false,
            body: body.into(),
            at_ms: 0,
            status: Status::Received,
            file: None,
        }
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

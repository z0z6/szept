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

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: String,
    pub mine: bool,
    pub body: String,
    pub at_ms: u64,
    pub status: Status,
}

impl Message {
    pub fn outgoing(id: String, body: String) -> Self {
        Self {
            id,
            mine: true,
            body,
            at_ms: now_ms(),
            status: Status::Queued,
        }
    }
}

#[derive(Debug, Default)]
pub struct Store {
    conversations: HashMap<String, Vec<Message>>,
    unread: HashMap<String, u32>,
}

impl Store {
    pub fn push(&mut self, peer: &str, message: Message) {
        let conversation = self.conversations.entry(peer.to_owned()).or_default();
        if !conversation.iter().any(|old| old.id == message.id) {
            conversation.push(message);
        }
    }

    /// Stores an incoming message. Returns `false` for a repeat of one
    /// already shown, which happens when the sender retries.
    pub fn receive(
        &mut self,
        peer: &str,
        id: String,
        at_ms: u64,
        body: String,
        seen: bool,
    ) -> bool {
        let conversation = self.conversations.entry(peer.to_owned()).or_default();
        if conversation.iter().any(|old| old.id == id) {
            return false;
        }
        conversation.push(Message {
            id,
            mine: false,
            body,
            at_ms,
            status: Status::Received,
        });
        if !seen {
            *self.unread.entry(peer.to_owned()).or_default() += 1;
        }
        true
    }

    /// Moves our message forward, never back: the confirmation may in theory
    /// overtake the report that the message left. Returns `true` on a change.
    pub fn set_status(&mut self, peer: &str, id: &str, status: Status) -> bool {
        let Some(message) = self
            .conversations
            .get_mut(peer)
            .and_then(|conversation| conversation.iter_mut().find(|message| message.id == id))
        else {
            return false;
        };
        if status.rank() <= message.status.rank() {
            return false;
        }
        message.status = status;
        true
    }

    pub fn conversation(&self, peer: &str) -> Vec<Message> {
        self.conversations.get(peer).cloned().unwrap_or_default()
    }

    pub fn last_body(&self, peer: &str) -> Option<String> {
        let message = self.conversations.get(peer)?.last()?;
        let mut preview: String = message.body.chars().take(60).collect();
        if preview.len() < message.body.len() {
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

    #[test]
    fn statuses_only_move_forward_and_repeats_are_dropped() {
        let mut store = Store::default();
        store.push("ola", Message::outgoing("1".into(), "hej".into()));
        assert!(store.set_status("ola", "1", Status::Delivered));
        assert!(!store.set_status("ola", "1", Status::Sent));
        assert_eq!(store.conversation("ola")[0].status, Status::Delivered);

        assert!(store.receive("ola", "2".into(), 0, "cześć 👋".into(), false));
        assert!(!store.receive("ola", "2".into(), 0, "cześć 👋".into(), false));
        assert_eq!(store.unread("ola"), 1);
        assert_eq!(store.last_body("ola").as_deref(), Some("cześć 👋"));
        assert!(store.mark_read("ola"));
        assert!(!store.mark_read("ola"));
    }
}

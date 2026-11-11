//! The wire protocol.
//!
//! Every frame travels on its own unidirectional QUIC stream: the sender
//! opens a stream, writes one encoded [`Frame`] and finishes the stream.

use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Protocol identifier negotiated in the QUIC handshake. Bump the trailing
/// number on every incompatible change.
pub const ALPN: &[u8] = b"komunikator/chat/0";

/// Upper bound for one encoded frame. Larger streams are rejected unread.
pub const MAX_FRAME_LEN: usize = 64 * 1024;

/// Upper bound for the text of one message, in bytes.
pub const MAX_TEXT_LEN: usize = 16 * 1024;

/// Random identifier of one message, chosen by the sender.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId([u8; 16]);

impl MessageId {
    fn random() -> Self {
        Self(rand::random())
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Debug for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MessageId({self})")
    }
}

/// A text message that is ready to be sent.
///
/// The identifier and the timestamp are fixed when the message is written, so
/// a message that waits in a queue is retried under the same identifier.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub id: MessageId,
    pub sent_at_ms: u64,
    pub body: String,
}

impl Outgoing {
    /// Prepares `body` for sending. Fails if it is empty or too long.
    pub fn new(body: impl Into<String>) -> Result<Self> {
        let body = body.into();
        ensure!(!body.is_empty(), "message is empty");
        ensure!(
            body.len() <= MAX_TEXT_LEN,
            "message is too long ({} bytes, the limit is {MAX_TEXT_LEN})",
            body.len()
        );
        Ok(Self {
            id: MessageId::random(),
            sent_at_ms: now_ms(),
            body,
        })
    }

    pub(crate) fn to_frame(&self) -> Frame {
        Frame::Text {
            id: self.id,
            sent_at_ms: self.sent_at_ms,
            body: self.body.clone(),
        }
    }
}

/// One unit sent over the wire.
///
/// New variants must be appended at the end: the position of a variant is
/// part of the encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Frame {
    /// A text message.
    Text {
        id: MessageId,
        sent_at_ms: u64,
        body: String,
    },
    /// Confirms that the message `id` reached the recipient's device.
    Ack { id: MessageId },
}

impl Frame {
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        let bytes = postcard::to_stdvec(self)?;
        ensure!(bytes.len() <= MAX_FRAME_LEN, "frame is too large");
        Ok(bytes)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let frame: Self = postcard::from_bytes(bytes)?;
        if let Self::Text { body, .. } = &frame {
            ensure!(!body.is_empty(), "empty message");
            ensure!(body.len() <= MAX_TEXT_LEN, "message is too long");
        }
        Ok(frame)
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
    fn frames_survive_a_round_trip() {
        let message = Outgoing::new("Zażółć gęślą jaźń 🦀").unwrap();
        for frame in [message.to_frame(), Frame::Ack { id: message.id }] {
            assert_eq!(Frame::decode(&frame.encode().unwrap()).unwrap(), frame);
        }
    }

    #[test]
    fn length_limits_are_enforced_on_both_sides() {
        assert!(Outgoing::new("").is_err());
        assert!(Outgoing::new("a".repeat(MAX_TEXT_LEN)).is_ok());
        assert!(Outgoing::new("a".repeat(MAX_TEXT_LEN + 1)).is_err());

        let oversized = Frame::Text {
            id: MessageId::random(),
            sent_at_ms: 0,
            body: "a".repeat(MAX_TEXT_LEN + 1),
        };
        let bytes = postcard::to_stdvec(&oversized).unwrap();
        assert!(Frame::decode(&bytes).is_err());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(Frame::decode(&[]).is_err());
        assert!(Frame::decode(&[0xff; 40]).is_err());
    }
}

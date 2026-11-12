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

/// Upper bound for the name of an offered file, in bytes.
pub const MAX_FILE_NAME_LEN: usize = 255;

/// Upper bound for a voice message or a GIF. The recipient's app fetches
/// these on its own, so they must stay small.
pub const MAX_MEDIA_SIZE: u64 = 16 * 1024 * 1024;

/// Upper bound for the length of one voice message.
pub const MAX_VOICE_MS: u32 = 30 * 60 * 1000;

/// Upper bound for a profile picture, in bytes. It travels inside one frame.
pub const MAX_AVATAR_LEN: usize = 48 * 1024;

/// Random identifier of one message, chosen by the sender.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId([u8; 16]);

impl MessageId {
    fn random() -> Self {
        Self(rand::random())
    }
}

impl std::str::FromStr for MessageId {
    type Err = anyhow::Error;

    /// Parses the 32 hex digits written by [`fmt::Display`].
    fn from_str(text: &str) -> Result<Self> {
        ensure!(text.len() == 32, "a message id has 32 hex digits");
        let mut bytes = [0u8; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)?;
        }
        Ok(Self(bytes))
    }
}

/// A file one contact makes available to another.
///
/// The file itself stays on the sender's device. The receiver fetches it
/// directly from there and checks it against `hash` before keeping it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOffer {
    /// The file name, without any directories.
    pub name: String,
    pub size: u64,
    /// BLAKE3 hash of the whole content.
    pub hash: [u8; 32],
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

/// What a file offer is for, when it is more than a plain file.
///
/// Media are small files the recipient's app fetches right away and shows
/// inside the conversation, instead of waiting for a tap on "download".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Media {
    /// A recorded voice message.
    Voice { duration_ms: u32 },
    /// An animated picture or a sticker, usually inserted from the keyboard.
    Gif,
}

impl Media {
    /// Returns `true` if a file called `name` can carry this kind of media.
    /// Pictures that can hold scripts, such as SVG, never pass.
    pub fn accepts(&self, name: &str) -> bool {
        let extension = name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default();
        match self {
            Self::Voice { duration_ms } => {
                *duration_ms <= MAX_VOICE_MS
                    && matches!(
                        extension.as_str(),
                        "m4a" | "aac" | "ogg" | "oga" | "opus" | "webm" | "mp3" | "wav"
                    )
            }
            Self::Gif => matches!(extension.as_str(), "gif" | "webp" | "png" | "jpg" | "jpeg"),
        }
    }
}

/// What a message carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    File(FileOffer),
    /// A voice message or a GIF: a file the recipient fetches at once.
    Media(FileOffer, Media),
}

/// A message that is ready to be sent.
///
/// The identifier and the timestamp are fixed when the message is written, so
/// a message that waits in a queue is retried under the same identifier.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub id: MessageId,
    pub sent_at_ms: u64,
    pub content: Content,
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
            content: Content::Text(body),
        })
    }

    /// Prepares an offer of a file. See [`crate::Node::share`].
    pub(crate) fn file(offer: FileOffer) -> Self {
        Self {
            id: MessageId::random(),
            sent_at_ms: now_ms(),
            content: Content::File(offer),
        }
    }

    /// Prepares an offer of a voice message or a GIF. See
    /// [`crate::Node::share_media`].
    pub(crate) fn media(offer: FileOffer, media: Media) -> Self {
        Self {
            id: MessageId::random(),
            sent_at_ms: now_ms(),
            content: Content::Media(offer, media),
        }
    }

    pub(crate) fn to_frame(&self) -> Frame {
        match &self.content {
            Content::Text(body) => Frame::Text {
                id: self.id,
                sent_at_ms: self.sent_at_ms,
                body: body.clone(),
            },
            Content::File(offer) => Frame::File {
                id: self.id,
                sent_at_ms: self.sent_at_ms,
                offer: offer.clone(),
            },
            Content::Media(offer, media) => Frame::Media {
                id: self.id,
                sent_at_ms: self.sent_at_ms,
                offer: offer.clone(),
                media: *media,
            },
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
    /// Offers a file, which the recipient may then fetch with the same `id`.
    File {
        id: MessageId,
        sent_at_ms: u64,
        offer: FileOffer,
    },
    /// Offers a voice message or a GIF, fetched like a file with the same
    /// `id`. Older versions of Szept drop this frame unread.
    Media {
        id: MessageId,
        sent_at_ms: u64,
        offer: FileOffer,
        media: Media,
    },
    /// The sender's profile picture (JPEG, PNG, WebP or GIF), or nothing
    /// when they removed it. Sent on every new connection.
    Avatar { image: Vec<u8> },
}

/// What the receiver of a file asks for on a fresh bidirectional stream: the
/// file offered as `id`, starting at byte `offset`. The answer is one status
/// byte (see [`FETCH_OK`]) followed by the raw bytes up to the end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FetchRequest {
    pub id: MessageId,
    pub offset: u64,
}

/// The file follows.
pub(crate) const FETCH_OK: u8 = 0;
/// Nothing was offered under this id to this contact, or it was withdrawn.
pub(crate) const FETCH_UNKNOWN: u8 = 1;
/// The file changed or disappeared on the sender's device.
pub(crate) const FETCH_GONE: u8 = 2;

impl Frame {
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        let bytes = postcard::to_stdvec(self)?;
        ensure!(bytes.len() <= MAX_FRAME_LEN, "frame is too large");
        Ok(bytes)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let frame: Self = postcard::from_bytes(bytes)?;
        match &frame {
            Self::Text { body, .. } => {
                ensure!(!body.is_empty(), "empty message");
                ensure!(body.len() <= MAX_TEXT_LEN, "message is too long");
            }
            Self::File { offer, .. } => {
                ensure!(crate::files::is_safe_name(&offer.name), "unsafe file name");
            }
            Self::Media { offer, media, .. } => {
                ensure!(crate::files::is_safe_name(&offer.name), "unsafe file name");
                ensure!(offer.size <= MAX_MEDIA_SIZE, "media file is too large");
                ensure!(media.accepts(&offer.name), "unexpected media file");
            }
            Self::Avatar { image } => {
                ensure!(image.len() <= MAX_AVATAR_LEN, "picture is too large");
                ensure!(
                    image.is_empty() || crate::files::is_picture(image),
                    "not a picture"
                );
            }
            Self::Ack { .. } => {}
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
        let file = Outgoing::file(FileOffer {
            name: "instalator.exe".into(),
            size: 7,
            hash: [9; 32],
        });
        let voice = Outgoing::media(
            FileOffer {
                name: "glosowa.ogg".into(),
                size: 3,
                hash: [1; 32],
            },
            Media::Voice { duration_ms: 1500 },
        );
        let gif = Outgoing::media(
            FileOffer {
                name: "kot.gif".into(),
                size: 3,
                hash: [2; 32],
            },
            Media::Gif,
        );
        for frame in [
            message.to_frame(),
            Frame::Ack { id: message.id },
            file.to_frame(),
            voice.to_frame(),
            gif.to_frame(),
            Frame::Avatar {
                image: b"\xff\xd8\xff\xe0 jpeg".to_vec(),
            },
            Frame::Avatar { image: Vec::new() },
        ] {
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
    fn message_ids_parse_back() {
        let id = MessageId::random();
        assert_eq!(id.to_string().parse::<MessageId>().unwrap(), id);
        assert!("zz".parse::<MessageId>().is_err());
    }

    #[test]
    fn offers_with_paths_in_the_name_are_rejected() {
        for name in ["../.bashrc", "a/b.txt", "C:\\x.exe", "", "."] {
            let frame = Frame::File {
                id: MessageId::random(),
                sent_at_ms: 0,
                offer: FileOffer {
                    name: name.into(),
                    size: 1,
                    hash: [0; 32],
                },
            };
            let bytes = postcard::to_stdvec(&frame).unwrap();
            assert!(Frame::decode(&bytes).is_err(), "{name} was accepted");
        }
    }

    #[test]
    fn media_must_be_small_and_of_the_right_type() {
        let media = |name: &str, size: u64, media: Media| {
            let frame = Frame::Media {
                id: MessageId::random(),
                sent_at_ms: 0,
                offer: FileOffer {
                    name: name.into(),
                    size,
                    hash: [0; 32],
                },
                media,
            };
            Frame::decode(&postcard::to_stdvec(&frame).unwrap())
        };
        let voice = Media::Voice { duration_ms: 10 };
        assert!(media("a.m4a", 10, voice).is_ok());
        assert!(media("a.webm", 10, voice).is_ok());
        assert!(media("a.gif", 10, Media::Gif).is_ok());
        assert!(media("a.exe", 10, voice).is_err());
        assert!(media("a.svg", 10, Media::Gif).is_err());
        assert!(media("a.gif", MAX_MEDIA_SIZE + 1, Media::Gif).is_err());
        let endless = Media::Voice {
            duration_ms: MAX_VOICE_MS + 1,
        };
        assert!(media("a.ogg", 10, endless).is_err());
    }

    #[test]
    fn avatars_must_be_small_pictures() {
        let decode =
            |image: Vec<u8>| Frame::decode(&postcard::to_stdvec(&Frame::Avatar { image }).unwrap());
        assert!(decode(Vec::new()).is_ok());
        assert!(decode(b"\x89PNG\r\n\x1a\n....".to_vec()).is_ok());
        assert!(decode(b"<svg onload=alert(1)>".to_vec()).is_err());
        let mut huge = b"\xff\xd8\xff".to_vec();
        huge.resize(MAX_AVATAR_LEN + 1, 0);
        assert!(decode(huge).is_err());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(Frame::decode(&[]).is_err());
        assert!(Frame::decode(&[0xff; 40]).is_err());
    }
}

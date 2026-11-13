//! The wire protocol.
//!
//! Every frame travels on its own unidirectional QUIC stream: the sender
//! opens a stream, writes one encoded [`Frame`] and finishes the stream.

use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, ensure};
use iroh::EndpointId;
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

/// Upper bound for a display name or a group name, in bytes.
pub const MAX_NAME_LEN: usize = 64;

/// Upper bound for the members of one group, the owner included.
pub const MAX_GROUP_MEMBERS: usize = 32;

/// Upper bound for the code of a reaction or a sticker, in bytes.
pub const MAX_CODE_LEN: usize = 32;

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

/// Random identifier of a group chat, chosen by its owner.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupId([u8; 16]);

impl GroupId {
    pub fn random() -> Self {
        Self(rand::random())
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Debug for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GroupId({self})")
    }
}

impl std::str::FromStr for GroupId {
    type Err = anyhow::Error;

    /// Parses the 32 hex digits written by [`fmt::Display`].
    fn from_str(text: &str) -> Result<Self> {
        ensure!(
            text.len() == 32 && text.is_ascii(),
            "a group id has 32 hex digits"
        );
        let mut bytes = [0u8; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)?;
        }
        Ok(Self(bytes))
    }
}

/// One person in a group: their key and the name the owner shows for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: [u8; 32],
    pub name: String,
}

impl Member {
    pub fn endpoint(&self) -> Option<EndpointId> {
        EndpointId::from_bytes(&self.id).ok()
    }
}

/// The state of a group, as its owner last announced it. Only the owner
/// changes it, and every change raises `version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupInfo {
    pub id: GroupId,
    pub owner: [u8; 32],
    pub version: u64,
    pub name: String,
    /// Everyone in the group, the owner included.
    pub members: Vec<Member>,
}

impl GroupInfo {
    pub fn owner(&self) -> Option<EndpointId> {
        EndpointId::from_bytes(&self.owner).ok()
    }

    pub fn has(&self, peer: &EndpointId) -> bool {
        self.members
            .iter()
            .any(|member| member.id == *peer.as_bytes())
    }

    /// Everyone except `me`.
    pub fn others(&self, me: &EndpointId) -> Vec<EndpointId> {
        self.members
            .iter()
            .filter(|member| member.id != *me.as_bytes())
            .filter_map(Member::endpoint)
            .collect()
    }

    fn check(&self) -> Result<()> {
        ensure!(is_name(&self.name), "bad group name");
        ensure!(
            !self.members.is_empty() && self.members.len() <= MAX_GROUP_MEMBERS,
            "bad number of group members"
        );
        ensure!(
            self.members.iter().any(|member| member.id == self.owner),
            "the owner is not a member"
        );
        for (index, member) in self.members.iter().enumerate() {
            ensure!(member.name.len() <= MAX_NAME_LEN, "member name too long");
            ensure!(member.endpoint().is_some(), "bad member key");
            ensure!(
                !self.members[..index]
                    .iter()
                    .any(|other| other.id == member.id),
                "member listed twice"
            );
        }
        ensure!(self.owner().is_some(), "bad owner key");
        Ok(())
    }
}

/// A non-empty name of at most [`MAX_NAME_LEN`] bytes, without control characters.
pub fn is_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= MAX_NAME_LEN && !name.chars().any(char::is_control)
}

/// A reaction or sticker code: short, lowercase letters, digits and dashes.
pub fn is_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= MAX_CODE_LEN
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// What a post carries. Posts are the newer kind of message: they can go to
/// a group, and they can be stickers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Post {
    Text(String),
    File {
        offer: FileOffer,
        media: Option<Media>,
    },
    /// One of the animated stickers every Szept app draws, by its code.
    Sticker(String),
}

impl Post {
    fn check(&self) -> Result<()> {
        match self {
            Self::Text(body) => {
                ensure!(!body.is_empty(), "empty message");
                ensure!(body.len() <= MAX_TEXT_LEN, "message is too long");
            }
            Self::File { offer, media } => {
                ensure!(crate::files::is_safe_name(&offer.name), "unsafe file name");
                if let Some(media) = media {
                    ensure!(offer.size <= MAX_MEDIA_SIZE, "media file is too large");
                    ensure!(media.accepts(&offer.name), "unexpected media file");
                }
            }
            Self::Sticker(code) => ensure!(is_code(code), "bad sticker code"),
        }
        Ok(())
    }
}

/// Something sent to a contact that is not a message of its own: it is not
/// confirmed, and it waits in the queue like a message while they are away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// A reaction to the message `target`; an empty `reaction` takes it back.
    React {
        target: MessageId,
        group: Option<GroupId>,
        reaction: String,
    },
    /// The current state of a group, from its owner.
    Group(GroupInfo),
    /// The sender left the group.
    Leave(GroupId),
    /// The sender's display name.
    Name(String),
}

impl Signal {
    pub(crate) fn to_frame(&self) -> Frame {
        match self {
            Self::React {
                target,
                group,
                reaction,
            } => Frame::React {
                target: *target,
                group: *group,
                reaction: reaction.clone(),
            },
            Self::Group(info) => Frame::Group { info: info.clone() },
            Self::Leave(group) => Frame::Leave { group: *group },
            Self::Name(name) => Frame::Name { name: name.clone() },
        }
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
    /// A post, for a group or a sticker.
    Post {
        group: Option<GroupId>,
        post: Post,
    },
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

    /// Prepares a post: a message for a group, or a sticker. Files in posts
    /// come from [`crate::Node::share_post`].
    pub fn post(group: Option<GroupId>, post: Post) -> Result<Self> {
        ensure!(
            !matches!(post, Post::File { .. }),
            "files are offered with Node::share_post"
        );
        post.check()?;
        Ok(Self::post_unchecked(group, post))
    }

    pub(crate) fn post_unchecked(group: Option<GroupId>, post: Post) -> Self {
        Self {
            id: MessageId::random(),
            sent_at_ms: now_ms(),
            content: Content::Post { group, post },
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
            Content::Post { group, post } => Frame::Post {
                id: self.id,
                sent_at_ms: self.sent_at_ms,
                group: *group,
                post: post.clone(),
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
    /// A post: a message for a group, or a sticker. Confirmed with [`Frame::Ack`].
    Post {
        id: MessageId,
        sent_at_ms: u64,
        group: Option<GroupId>,
        post: Post,
    },
    /// A reaction to a message; empty to take it back.
    React {
        target: MessageId,
        group: Option<GroupId>,
        reaction: String,
    },
    /// The state of a group, from its owner.
    Group { info: GroupInfo },
    /// The sender left a group.
    Leave { group: GroupId },
    /// The sender's display name.
    Name { name: String },
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
            Self::Post { post, .. } => post.check()?,
            Self::React { reaction, .. } => {
                ensure!(reaction.is_empty() || is_code(reaction), "bad reaction");
            }
            Self::Group { info } => info.check()?,
            Self::Name { name } => ensure!(is_name(name), "bad name"),
            Self::Ack { .. } | Self::Leave { .. } => {}
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
    fn posts_and_signals_survive_a_round_trip() {
        let me = iroh::SecretKey::generate().public();
        let ola = iroh::SecretKey::generate().public();
        let group = GroupId::random();
        let info = GroupInfo {
            id: group,
            owner: *me.as_bytes(),
            version: 3,
            name: "Wycieczka".into(),
            members: vec![
                Member {
                    id: *me.as_bytes(),
                    name: "Ala".into(),
                },
                Member {
                    id: *ola.as_bytes(),
                    name: "Ola".into(),
                },
            ],
        };
        assert_eq!(info.others(&me), vec![ola]);
        let text = Outgoing::post(Some(group), Post::Text("cześć grupo".into())).unwrap();
        let sticker = Outgoing::post(None, Post::Sticker("pierog".into())).unwrap();
        for frame in [
            text.to_frame(),
            sticker.to_frame(),
            Signal::React {
                target: text.id,
                group: Some(group),
                reaction: "ogorek".into(),
            }
            .to_frame(),
            Signal::Group(info.clone()).to_frame(),
            Signal::Leave(group).to_frame(),
            Signal::Name("Ala z Mokotowa".into()).to_frame(),
        ] {
            assert_eq!(Frame::decode(&frame.encode().unwrap()).unwrap(), frame);
        }
        assert_eq!(group.to_string().parse::<GroupId>().unwrap(), group);

        assert!(Outgoing::post(None, Post::Sticker("Zły Kod!".into())).is_err());
        assert!(Outgoing::post(None, Post::Text(String::new())).is_err());
        let mut broken = info.clone();
        broken.members.retain(|member| member.id != info.owner);
        let frame = Frame::Group { info: broken };
        assert!(Frame::decode(&postcard::to_stdvec(&frame).unwrap()).is_err());
        let frame = Frame::Name {
            name: "x".repeat(MAX_NAME_LEN + 1),
        };
        assert!(Frame::decode(&postcard::to_stdvec(&frame).unwrap()).is_err());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(Frame::decode(&[]).is_err());
        assert!(Frame::decode(&[0xff; 40]).is_err());
    }
}

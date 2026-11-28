//! System notifications: who wrote, with their picture, and a short preview
//! of what they wrote, unless the person turned previews off. The app plays
//! its own sound, so the system's stays silent.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::store::Message;

/// The longest preview, in characters.
const PREVIEW_CHARS: usize = 90;

/// What `notify.json` keeps. The windows keep the same choices for
/// themselves; this copy is for notifications that come before any window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Show a short preview of the message, not only who sent it.
    pub preview: bool,
    /// The sound for new messages, from tools/sounds.py; "none" for silence.
    pub sound: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            preview: true,
            sound: "babel".into(),
        }
    }
}

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("notify.json")
}

pub fn load(data_dir: &Path) -> Settings {
    fs::read(settings_path(data_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(data_dir: &Path, settings: &Settings) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    fs::write(settings_path(data_dir), json)
}

/// What a notification says about `message`: its text, shortened and on
/// one line, or what kind of thing it is.
pub fn preview(message: &Message) -> String {
    if let Some(payment) = &message.payment {
        return format!("Ӿ {} XNO", payment.amount);
    }
    if message.sticker.is_some() {
        return crate::i18n::t("Naklejka 🥟");
    }
    if let Some(file) = &message.file {
        return match file.media {
            Some("voice") => crate::i18n::t("🎤 Wiadomość głosowa"),
            Some("gif") => "GIF".into(),
            _ => format!("📎 {}", file.name),
        };
    }
    shorten(&message.body)
}

/// One line of at most [`PREVIEW_CHARS`] characters, cut at a word.
pub fn shorten(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= PREVIEW_CHARS {
        return line;
    }
    let cut: String = line.chars().take(PREVIEW_CHARS).collect();
    let cut = match cut.rfind(' ') {
        Some(space) if space > PREVIEW_CHARS / 2 => &cut[..space],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', '.', ';', ':', '-']))
}

/// A picture for the notification: the person's profile picture, saved
/// where the notification service can read it. Only JPEG and PNG, which
/// every system shows.
pub fn icon(dir: &Path, id: &str, picture: Option<Vec<u8>>) -> Option<PathBuf> {
    let picture = picture?;
    let extension = if picture.starts_with(b"\x89PNG") {
        "png"
    } else if picture.starts_with(b"\xff\xd8\xff") {
        "jpg"
    } else {
        return None;
    };
    // Identifiers are hex, so they are safe as file names.
    if !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let path = dir.join(format!("{id}.{extension}"));
    if fs::read(&path).ok().as_deref() != Some(picture.as_slice()) {
        fs::create_dir_all(dir).ok()?;
        fs::write(&path, &picture).ok()?;
    }
    Some(path)
}

/// One notification per conversation: a new one replaces the last.
fn notification_id(key: &str) -> u32 {
    key.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    }) | 1
}

/// Shows a notification for the conversation `key` in the background.
pub fn show(identifier: String, key: &str, title: String, body: String, icon: Option<PathBuf>) {
    let id = notification_id(key);
    tauri::async_runtime::spawn_blocking(move || {
        let mut notification = notify_rust::Notification::new();
        notification
            .appname("Szept")
            .summary(&title)
            .body(&body)
            .id(id);
        match &icon {
            Some(path) => {
                let path = path.to_string_lossy();
                notification.icon(&path).image_path(&path);
            }
            None => {
                notification.auto_icon();
            }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // Szept plays its own sound.
            notification
                .hint(notify_rust::Hint::SuppressSound(true))
                .hint(notify_rust::Hint::Category("im.received".into()));
        }
        #[cfg(windows)]
        {
            // Without a sound name a toast is silent. The app id makes the
            // toast show up as Szept, but only for the installed app.
            let installed = tauri::utils::platform::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
                .is_some_and(|dir| {
                    !dir.ends_with("target/debug") && !dir.ends_with("target/release")
                });
            if installed {
                notification.app_id(&identifier);
            }
        }
        #[cfg(target_os = "macos")]
        {
            let _ = notify_rust::set_application(&identifier);
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = identifier;
        let _ = notification.show();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_short_single_lines() {
        assert_eq!(shorten("  cześć\n\nco  tam? "), "cześć co tam?");
        let long = "słowo ".repeat(40);
        let short = shorten(&long);
        assert!(short.ends_with('…'));
        assert!(short.chars().count() <= PREVIEW_CHARS + 1);
        assert!(!short.contains("  "));
        let one = "x".repeat(200);
        assert_eq!(shorten(&one).chars().count(), PREVIEW_CHARS + 1);
    }

    #[test]
    fn only_jpeg_and_png_become_icons() {
        let dir = std::env::temp_dir().join(format!("szept-icons-{}", std::process::id()));
        let id = "ab".repeat(32);
        let png = b"\x89PNG\r\n\x1a\n....".to_vec();
        let path = icon(&dir, &id, Some(png.clone())).unwrap();
        assert_eq!(fs::read(&path).unwrap(), png);
        assert!(icon(&dir, &id, Some(b"RIFF....WEBP".to_vec())).is_none());
        assert!(icon(&dir, "../x", Some(png)).is_none());
        assert!(icon(&dir, &id, None).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn each_conversation_keeps_its_own_notification() {
        assert_ne!(notification_id("a"), notification_id("b"));
        assert_eq!(notification_id("a"), notification_id("a"));
        assert_ne!(notification_id(""), 0);
    }
}

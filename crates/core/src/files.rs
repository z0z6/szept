//! Local side of file sharing: checking names that come from the network,
//! hashing files and choosing where a download lands.

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::protocol::{FileOffer, MAX_FILE_NAME_LEN};

/// Names Windows refuses for files, in any letter case and with any extension.
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Returns `true` if `name` is a plain file name that is safe to create on
/// Linux, Windows, macOS and Android: no directories, no device names, no
/// characters any of them reject.
///
/// Names come from the other device, so they are checked before anything is
/// written. A name that fails is refused, not repaired: the sender's app only
/// offers names that pass.
pub fn is_safe_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_FILE_NAME_LEN {
        return false;
    }
    if name == "." || name == ".." || name.starts_with(' ') || name.ends_with([' ', '.']) {
        return false;
    }
    let bad_char = |c: char| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    };
    if name.chars().any(bad_char) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or_default();
    !WINDOWS_RESERVED
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

/// Turns a local file name into one that passes [`is_safe_name`].
pub fn safe_name(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned = cleaned.trim_matches([' ', '.']).to_owned();
    // Shorten an overlong name, keeping a reasonable extension.
    if cleaned.len() > MAX_FILE_NAME_LEN {
        let extension = cleaned
            .rfind('.')
            .map(|dot| cleaned[dot..].to_owned())
            .filter(|ext| ext.len() <= 32)
            .unwrap_or_default();
        let mut cut = MAX_FILE_NAME_LEN - extension.len();
        while !cleaned.is_char_boundary(cut) {
            cut -= 1;
        }
        cleaned.truncate(cut);
        cleaned.push_str(&extension);
    }
    if cleaned.is_empty() {
        return "plik".to_owned();
    }
    if !is_safe_name(&cleaned) {
        cleaned = format!("plik_{cleaned}");
    }
    if is_safe_name(&cleaned) {
        cleaned
    } else {
        "plik".to_owned()
    }
}

/// Describes a local file for sending: its safe name, size and hash.
///
/// Reads the whole file, so call it off the main thread.
pub fn describe(path: &Path) -> Result<FileOffer> {
    let file_name = path
        .file_name()
        .with_context(|| format!("{} is not a file", path.display()))?
        .to_string_lossy();
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("{} is not a file", path.display());
    }
    let mut hasher = blake3::Hasher::new();
    let size = copy_into_hasher(&mut file, &mut hasher)?;
    Ok(FileOffer {
        name: safe_name(&file_name),
        size,
        hash: *hasher.finalize().as_bytes(),
    })
}

/// Feeds everything `reader` yields into `hasher`. Returns the byte count.
pub(crate) fn copy_into_hasher(
    reader: &mut impl Read,
    hasher: &mut blake3::Hasher,
) -> io::Result<u64> {
    let mut buffer = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => return Ok(total),
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
}

/// A path in `dir` for `name` that does not exist yet: `name`, then
/// `name (1)`, `name (2)` and so on, keeping the extension at the end.
pub(crate) fn free_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    (1..)
        .map(|number| dir.join(format!("{stem} ({number}){extension}")))
        .find(|path| !path.exists())
        .expect("some number is free")
}

/// What kind of file a name suggests, judged by its extension: "program",
/// "archive", "image", "audio", "video", "document" or "other".
///
/// Programs are never opened by the app itself: the person runs them from
/// the folder, knowingly.
pub fn kind(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "exe" | "msi" | "msix" | "appx" | "bat" | "cmd" | "com" | "scr" | "ps1" | "vbs" | "js"
        | "jar" | "apk" | "aab" | "xapk" | "apks" | "deb" | "rpm" | "appimage" | "sh" | "run"
        | "bin" | "dmg" | "pkg" | "app" | "flatpak" | "snap" | "lnk" | "reg" | "desktop" => {
            "program"
        }
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" => "archive",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "heic" | "heif" | "avif"
        | "tif" | "tiff" | "ico" => "image",
        "mp3" | "flac" | "ogg" | "oga" | "opus" | "wav" | "m4a" | "aac" | "wma" | "aiff"
        | "aif" | "mid" | "midi" => "audio",
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "wmv" | "3gp" => "video",
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "xls" | "xlsx" | "ods" | "ppt"
        | "pptx" | "odp" | "epub" | "csv" => "document",
        _ => "other",
    }
}

/// Formats a hash the way people compare them: lowercase hex.
pub fn hash_hex(hash: &[u8; 32]) -> String {
    blake3::Hash::from_bytes(*hash).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangerous_names_are_refused() {
        for name in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "a\\b",
            "C:x",
            "con",
            "Nul.txt",
            "lpt1.exe",
            "x\n.sh",
            "trailing.",
            " leading",
            "a?b",
            "a|b",
        ] {
            assert!(!is_safe_name(name), "{name:?} passed");
        }
        for name in [
            "setup.exe",
            "szept.apk",
            "muzyka – mix 🎵.flac",
            ".bashrc",
            "con.d.txt2",
        ] {
            // "con.d" starts with a reserved stem, so only the others pass.
            assert_eq!(is_safe_name(name), name != "con.d.txt2", "{name:?}");
        }
    }

    #[test]
    fn local_names_are_cleaned_up() {
        assert_eq!(
            safe_name("raport: wersja 2?.docx"),
            "raport_ wersja 2_.docx"
        );
        assert_eq!(safe_name("..."), "plik");
        assert_eq!(safe_name("CON"), "plik_CON");
        let long = format!("{}.mp3", "a".repeat(400));
        let short = safe_name(&long);
        assert!(short.len() <= MAX_FILE_NAME_LEN && short.ends_with(".mp3"));
    }

    #[test]
    fn kinds_follow_the_extension() {
        assert_eq!(kind("Setup.EXE"), "program");
        assert_eq!(kind("szept.apk"), "program");
        assert_eq!(kind("pakiet.deb"), "program");
        assert_eq!(kind("a.tar.gz"), "archive");
        assert_eq!(kind("raport.docx"), "document");
        assert_eq!(kind("mix.flac"), "audio");
        assert_eq!(kind("foto.HEIC"), "image");
        assert_eq!(kind("README"), "other");
    }

    #[test]
    fn free_paths_do_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("szept-free-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.zip"), dir.join("a.zip"));
        std::fs::write(dir.join("a.zip"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.zip"), dir.join("a (1).zip"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

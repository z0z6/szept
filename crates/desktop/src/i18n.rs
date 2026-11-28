//! The few texts the Rust side shows itself: notifications, file dialogs,
//! window titles and some errors. Translations come from `i18n/*.json` at
//! the root of the repository, the same files the windows and Android use;
//! the key is the Polish text, which is also what shows when a translation
//! is missing.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

use crate::i18n_data::TRANSLATIONS;

type Table = HashMap<String, String>;

fn tables() -> &'static HashMap<&'static str, Table> {
    static TABLES: OnceLock<HashMap<&'static str, Table>> = OnceLock::new();
    TABLES.get_or_init(|| {
        TRANSLATIONS
            .iter()
            .map(|(code, json)| (*code, serde_json::from_str(json).unwrap_or_default()))
            .collect()
    })
}

/// The language in use, as a code from `i18n/`, or "pl".
static LANGUAGE: RwLock<String> = RwLock::new(String::new());

pub fn language() -> String {
    let language = LANGUAGE.read().unwrap();
    if language.is_empty() {
        "pl".into()
    } else {
        language.clone()
    }
}

/// Switches the language; unknown codes mean Polish.
pub fn set(code: &str) {
    let code = if code == "pl" || tables().contains_key(code) {
        code
    } else {
        "pl"
    };
    *LANGUAGE.write().unwrap() = code.to_owned();
}

/// `text` in the language in use.
pub fn t(text: &str) -> String {
    let language = LANGUAGE.read().unwrap();
    tables()
        .get(language.as_str())
        .and_then(|table| table.get(text))
        .cloned()
        .unwrap_or_else(|| text.to_owned())
}

/// [`t`], then each `{name}` replaced by its value.
pub fn tf(text: &str, args: &[(&str, &str)]) -> String {
    let mut out = t(text);
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("language.txt")
}

/// The language the windows chose last time, for what shows before they open.
pub fn load(data_dir: &Path) {
    if let Ok(code) = fs::read_to_string(path(data_dir)) {
        set(code.trim());
    }
}

pub fn save(data_dir: &Path, code: &str) -> std::io::Result<()> {
    set(code);
    fs::write(path(data_dir), language())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_file_parses_and_is_complete() {
        let keys = TRANSLATIONS.iter().map(|(code, _)| (*code, tables()[code].len()));
        let counts: Vec<(&str, usize)> = keys.collect();
        assert_eq!(counts.len(), 10);
        let first = counts[0].1;
        assert!(first > 400, "{counts:?}");
        assert!(counts.iter().all(|(_, count)| *count == first), "{counts:?}");
        assert_eq!(tables()["en"]["Nowa wiadomość"], "New message");
    }

    #[test]
    fn missing_translations_fall_back_to_polish() {
        assert_eq!(tf("Ty: {preview}", &[("preview", "hej")]), "Ty: hej");
        let unknown = "Nie ma takiego tekstu w żadnym słowniku";
        assert_eq!(t(unknown), unknown);
    }
}

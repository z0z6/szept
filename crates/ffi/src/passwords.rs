//! Passwords for archives: how strong one looks, and a strong one to suggest.
//!
//! The estimate is deliberately simple and conservative. It counts the kinds
//! of characters used, ignores repeats and runs like "1234" or "abcd", and
//! knows the passwords everyone tries first. It cannot know that a phrase
//! is a famous quote, so the generator is the safer choice.

use std::sync::OnceLock;

/// The shortest password accepted for an archive.
pub const MIN_PASSWORD_LEN: usize = 12;

/// The estimated strength an archive password must reach.
pub const MIN_PASSWORD_BITS: u32 = 56;

/// How strong a password looks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, uniffi::Record)]
pub struct PasswordStrength {
    /// 0 very weak, 1 weak, 2 fair, 3 good, 4 very good.
    pub score: u8,
    /// Estimated bits of entropy.
    pub bits: u32,
    /// Long and strong enough for an archive.
    pub acceptable: bool,
}

/// Rates `password` for use with an archive.
#[uniffi::export]
pub fn password_strength(password: String) -> PasswordStrength {
    let bits = estimate_bits(&password);
    let score = match bits {
        0..28 => 0,
        28..40 => 1,
        40..MIN_PASSWORD_BITS => 2,
        MIN_PASSWORD_BITS..72 => 3,
        _ => 4,
    };
    PasswordStrength {
        score,
        bits,
        acceptable: password.chars().count() >= MIN_PASSWORD_LEN && bits >= MIN_PASSWORD_BITS,
    }
}

/// Suggests a password that is easy to write down and hard to guess: seven
/// random words from a list of 256 and a three-digit number, about 66 bits.
#[uniffi::export]
pub fn suggest_passphrase() -> String {
    let mut parts: Vec<String> = (0..7)
        .map(|_| WORDS[usize::from(rand::random::<u8>())].to_owned())
        .collect();
    parts.push((100 + rand::random::<u32>() % 900).to_string());
    parts.join("-")
}

/// The shortest password accepted for an archive, for the interface.
#[uniffi::export]
pub fn min_password_len() -> u32 {
    MIN_PASSWORD_LEN as u32
}

pub(crate) fn estimate_bits(password: &str) -> u32 {
    let chars: Vec<char> = password.chars().collect();
    if chars.is_empty() {
        return 0;
    }
    let lower = password.to_lowercase();
    // A well-known password, maybe with digits or signs around it.
    let core = lower.trim_matches(|c: char| !c.is_alphabetic());
    if common()
        .iter()
        .any(|known| lower == *known || core == *known)
    {
        let extra = chars.len().saturating_sub(core.chars().count()) as u32;
        return 8 + extra * 3;
    }

    let (mut lowercase, mut uppercase, mut digits, mut symbols, mut other) =
        (false, false, false, false, false);
    for c in &chars {
        match c {
            'a'..='z' => lowercase = true,
            'A'..='Z' => uppercase = true,
            '0'..='9' => digits = true,
            c if c.is_ascii() => symbols = true,
            _ => other = true,
        }
    }
    let pool: u32 = [
        (lowercase, 26),
        (uppercase, 26),
        (digits, 10),
        (symbols, 33),
        (other, 40),
    ]
    .iter()
    .filter(|(used, _)| *used)
    .map(|(_, size)| size)
    .sum();

    // Repeats ("aaaa") and runs ("1234", "abcd", "dcba") add almost nothing.
    let mut effective = 1.0f64;
    for pair in chars.windows(2) {
        let step = pair[1] as i64 - pair[0] as i64;
        effective += if step.abs() <= 1 { 0.15 } else { 1.0 };
    }
    let bits = effective * f64::from(pool.max(2)).log2();
    bits.floor().min(f64::from(u32::MAX)) as u32
}

fn common() -> &'static [&'static str] {
    static COMMON: OnceLock<Vec<&'static str>> = OnceLock::new();
    COMMON.get_or_init(|| {
        [
            "haslo",
            "password",
            "qwerty",
            "qwertyuiop",
            "asdfgh",
            "zxcvbn",
            "zaq",
            "zaqwsx",
            "qazwsx",
            "admin",
            "administrator",
            "polska",
            "kochanie",
            "kocham",
            "kochamcie",
            "dragon",
            "monkey",
            "iloveyou",
            "letmein",
            "welcome",
            "football",
            "pilka",
            "matrix",
            "superman",
            "batman",
            "szept",
            "szeptszept",
            "komunikator",
            "abc",
            "abcd",
            "abcdef",
            "qwe",
            "mama",
            "tata",
            "marcin",
            "michal",
            "agnieszka",
            "kasia",
            "anna",
            "piotr",
            "tomek",
            "lato",
            "zima",
            "wiosna",
            "jesien",
            "slonce",
            "sloneczko",
            "misiek",
            "myszka",
            "kotek",
            "piesek",
            "legia",
            "lech",
            "wisla",
            "polska",
            "warszawa",
            "krakow",
        ]
        .to_vec()
    })
}

const WORDS: [&str; 256] = [
    "agrest",
    "akacja",
    "album",
    "alpaka",
    "arbuz",
    "atlas",
    "autobus",
    "balkon",
    "banan",
    "barka",
    "baton",
    "bazar",
    "beczka",
    "bieg",
    "biurko",
    "blok",
    "bluza",
    "bocian",
    "brama",
    "brzoza",
    "budka",
    "burak",
    "butelka",
    "cebula",
    "cegla",
    "chata",
    "chleb",
    "chmura",
    "ciasto",
    "cytryna",
    "czapka",
    "czajnik",
    "dach",
    "deska",
    "deszcz",
    "dom",
    "drabina",
    "drozd",
    "dywan",
    "dzban",
    "dzwon",
    "fala",
    "farba",
    "figa",
    "foka",
    "fotel",
    "gazeta",
    "gitara",
    "glina",
    "gniazdo",
    "golab",
    "gora",
    "gruszka",
    "guzik",
    "herbata",
    "hotel",
    "igla",
    "indyk",
    "jablko",
    "jagoda",
    "jaskolka",
    "jezioro",
    "jodla",
    "kaczka",
    "kajak",
    "kalosz",
    "kamien",
    "kanapa",
    "kapusta",
    "karton",
    "kasza",
    "kawa",
    "kielich",
    "klucz",
    "kolano",
    "komin",
    "konewka",
    "korale",
    "koszyk",
    "kotlet",
    "krawat",
    "kreda",
    "kubek",
    "kufer",
    "kwiat",
    "lampa",
    "latarka",
    "lawenda",
    "lew",
    "lisek",
    "lodka",
    "lody",
    "lustro",
    "malina",
    "mapa",
    "marchew",
    "maslo",
    "mewa",
    "miotla",
    "miska",
    "morze",
    "most",
    "motyl",
    "mleko",
    "mrowka",
    "muszla",
    "namiot",
    "noga",
    "nozyczki",
    "obraz",
    "ogrod",
    "okno",
    "olowek",
    "orzech",
    "owca",
    "palma",
    "papuga",
    "parasol",
    "pasek",
    "pestka",
    "piasek",
    "pianino",
    "pieczarka",
    "piorko",
    "pizama",
    "plaster",
    "plecak",
    "pomidor",
    "pociag",
    "poduszka",
    "pomarancza",
    "prom",
    "puszka",
    "rabarbar",
    "rakieta",
    "ratusz",
    "rekawica",
    "robot",
    "rower",
    "rybka",
    "rzeka",
    "sanki",
    "serce",
    "sikorka",
    "skrzynka",
    "slonce",
    "sowa",
    "spodnie",
    "stol",
    "strych",
    "suknia",
    "szafa",
    "szalik",
    "szczotka",
    "szklanka",
    "szuflada",
    "tarcza",
    "teczka",
    "tort",
    "traktor",
    "trawa",
    "tulipan",
    "tygrys",
    "walizka",
    "wazon",
    "wiadro",
    "widelec",
    "wiewiorka",
    "winogrono",
    "wydra",
    "zamek",
    "zebra",
    "zegar",
    "zeszyt",
    "zielnik",
    "zupa",
    "zabka",
    "akwarium",
    "baranek",
    "bilet",
    "borsuk",
    "bratek",
    "budzik",
    "chomik",
    "cukier",
    "cyrk",
    "dynia",
    "fasola",
    "foremka",
    "grzebien",
    "grzyb",
    "hamak",
    "harmonia",
    "jamnik",
    "jelen",
    "jezyk",
    "kaktus",
    "kalendarz",
    "kamyk",
    "kaczor",
    "kangur",
    "kasztan",
    "kino",
    "kogut",
    "kolejka",
    "komar",
    "konik",
    "kosz",
    "krolik",
    "ksiazka",
    "kurtka",
    "lalka",
    "latawiec",
    "lizak",
    "lotnia",
    "lukier",
    "magnes",
    "malpa",
    "mlyn",
    "modrzew",
    "nalesnik",
    "notes",
    "obiad",
    "ocean",
    "oliwka",
    "orkiestra",
    "paczka",
    "pajak",
    "pantera",
    "pelikan",
    "pierog",
    "pingwin",
    "plaza",
    "pomnik",
    "prezent",
    "pudelko",
    "radio",
    "rogal",
    "salata",
    "sandal",
    "sarna",
    "siano",
    "skarpeta",
    "sliwka",
    "smok",
    "sok",
    "sosna",
    "stadion",
    "szachy",
    "szpinak",
    "szyszka",
    "taras",
    "telefon",
    "tramwaj",
    "truskawka",
    "wagon",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_passwords_are_rejected() {
        for weak in [
            "",
            "haslo",
            "Haslo123!",
            "qwerty123456",
            "aaaaaaaaaaaaaaaa",
            "123456789012",
            "Kotek12345!!",
        ] {
            assert!(!password_strength(weak.into()).acceptable, "{weak}");
        }
    }

    #[test]
    fn strong_passwords_and_suggestions_pass() {
        for strong in ["kot-rower-jablko-most-17", "Zielony8Most#Pada&Rzeka"] {
            assert!(password_strength(strong.into()).acceptable, "{strong}");
        }
        for _ in 0..20 {
            let suggestion = suggest_passphrase();
            assert_eq!(suggestion.split('-').count(), 8);
            let strength = password_strength(suggestion);
            assert!(strength.acceptable);
            assert_eq!(strength.score, 4);
        }
    }

    #[test]
    fn the_word_list_has_no_repeats() {
        let mut sorted = WORDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 256);
    }
}

//! The wallet seed and its 24 words.

use anyhow::{Context, Result, bail};
use blake2::{Blake2b, Digest, digest::consts::U32};
use zeroize::Zeroizing;

use crate::{PrivateKey, from_hex, hex};

/// A Nano seed: 32 random bytes, wiped from memory when dropped.
pub struct Seed(Zeroizing<[u8; 32]>);

impl Seed {
    /// A fresh seed from the operating system's random generator.
    pub fn generate() -> Self {
        Self(Zeroizing::new(rand::random()))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The 64 hex digits other wallets also accept.
    pub fn to_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex(&*self.0))
    }

    /// The 24 BIP39 words (English list) with the seed as entropy.
    pub fn words(&self) -> Vec<Zeroizing<String>> {
        let mnemonic = bip39::Mnemonic::from_entropy(&*self.0).expect("32 bytes give 24 words");
        mnemonic
            .words()
            .map(|word| Zeroizing::new(word.to_owned()))
            .collect()
    }

    /// Reads a backup: 24 words, or the seed as 64 hex digits. Case, extra
    /// spaces, commas and numbering ("1. abandon 2. ...") do not matter.
    pub fn parse(text: &str) -> Result<Self> {
        let trimmed = text.trim();
        if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(Self::from_bytes(from_hex(trimmed)?));
        }
        let words: Vec<String> = trimmed
            .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
            .map(|word| word.trim_matches(|c: char| !c.is_alphabetic()))
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect();
        if words.len() != 24 {
            bail!(
                "a Nano backup has 24 words or 64 hex digits (found {} words)",
                words.len()
            );
        }
        let phrase = Zeroizing::new(words.join(" "));
        let mnemonic = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &phrase)
            .context("these words are not a valid backup (a typo, or a word out of place)")?;
        let (entropy, len) = mnemonic.to_entropy_array();
        let entropy = Zeroizing::new(entropy);
        let bytes: [u8; 32] = entropy[..len]
            .try_into()
            .context("a Nano backup has 24 words")?;
        Ok(Self::from_bytes(bytes))
    }

    /// The private key of account `index`.
    pub fn key(&self, index: u32) -> PrivateKey {
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(*self.0);
        hasher.update(index.to_be_bytes());
        let key = Zeroizing::new(<[u8; 32]>::from(hasher.finalize()));
        PrivateKey::from_bytes(&key)
    }
}

impl Clone for Seed {
    fn clone(&self) -> Self {
        Self::from_bytes(*self.0)
    }
}

impl PartialEq for Seed {
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl std::fmt::Debug for Seed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Seed(…)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zero_seed_gives_the_documented_account() {
        // https://docs.nano.org/integration-guides/key-management/
        let seed = Seed::from_bytes([0; 32]);
        let key = seed.key(0);
        assert_eq!(
            hex(&key.secret_bytes()),
            "9F0E444C69F77A49BD0BE89DB92C38FE713E0963165CCA12FAF5712D7657120F"
        );
        assert_eq!(
            key.account().to_string(),
            "nano_3i1aq1cchnmbn9x5rsbap8b15akfh7wj7pwskuzi7ahz8oq6cobd99d4r3b7"
        );
    }

    #[test]
    fn words_go_both_ways() {
        let zero = Seed::from_bytes([0; 32]);
        let words: Vec<String> = zero.words().iter().map(|w| w.to_string()).collect();
        assert_eq!(words.len(), 24);
        assert!(words[..23].iter().all(|w| w == "abandon"));
        assert_eq!(words[23], "art");

        let seed = Seed::generate();
        let text = seed
            .words()
            .iter()
            .enumerate()
            .map(|(i, w)| format!("{}. {}", i + 1, w.to_uppercase()))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(Seed::parse(&text).unwrap(), seed);
        assert_eq!(Seed::parse(&seed.to_hex().to_lowercase()).unwrap(), seed);
    }

    #[test]
    fn the_docs_example_mnemonic_matches() {
        // The Nano docs map the seed 00…01 to a phrase ending in "diesel".
        let mut bytes = [0u8; 32];
        bytes[31] = 1;
        let words = Seed::from_bytes(bytes).words();
        assert_eq!(words.last().unwrap().as_str(), "diesel");
    }

    #[test]
    fn bad_backups_are_rejected() {
        assert!(Seed::parse("abandon abandon").is_err());
        let mut words = vec!["abandon"; 24];
        assert!(Seed::parse(&words.join(" ")).is_err(), "bad checksum");
        words[23] = "art";
        assert!(Seed::parse(&words.join(" ")).is_ok());
        words[3] = "szept";
        assert!(Seed::parse(&words.join(" ")).is_err(), "not on the list");
    }
}

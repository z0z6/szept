//! Keys and `nano_` addresses.

use std::{fmt, str::FromStr};

use anyhow::{Result, bail, ensure};
use blake2::{Blake2b, Blake2b512, Digest, digest::consts::U5};
use ed25519_dalek::{
    Signature, VerifyingKey,
    hazmat::{ExpandedSecretKey, raw_sign, raw_verify},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The alphabet of Nano addresses: no 0, 2, l or v.
const ALPHABET: &[u8; 32] = b"13456789abcdefghijkmnopqrstuwxyz";

/// A Nano account: its public key, written as a `nano_` address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Account(pub [u8; 32]);

impl Account {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Checks an Ed25519-Blake2b signature made by this account.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        let Ok(key) = VerifyingKey::from_bytes(&self.0) else {
            return false;
        };
        raw_verify::<Blake2b512>(&key, message, &Signature::from_bytes(signature)).is_ok()
    }

    /// Accepts a bare address and a `nano:` payment link, ignoring anything
    /// after `?` (an amount, a label).
    pub fn parse_link(text: &str) -> Result<Self> {
        let text = text.trim();
        let text = match text.get(..5) {
            Some(prefix) if prefix.eq_ignore_ascii_case("nano:") => &text[5..],
            _ => text,
        };
        let text = text.split('?').next().unwrap_or_default();
        text.parse()
    }
}

fn checksum(key: &[u8; 32]) -> [u8; 5] {
    let mut sum: [u8; 5] = Blake2b::<U5>::digest(key).into();
    sum.reverse();
    sum
}

impl fmt::Display for Account {
    /// 4 zero bits, the 256-bit key and a 40-bit checksum, in 60 base-32 digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bits: Vec<bool> = vec![false; 4];
        for byte in self.0.iter().chain(checksum(&self.0).iter()) {
            bits.extend((0..8).rev().map(|shift| (byte >> shift) & 1 == 1));
        }
        f.write_str("nano_")?;
        for chunk in bits.chunks(5) {
            let digit = chunk
                .iter()
                .fold(0usize, |acc, &bit| acc * 2 + usize::from(bit));
            write!(f, "{}", ALPHABET[digit] as char)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Account({self})")
    }
}

impl FromStr for Account {
    type Err = anyhow::Error;

    /// Parses `nano_…` (or the old `xrb_…`) and checks the checksum, so a
    /// mistyped address is caught before any money moves.
    fn from_str(text: &str) -> Result<Self> {
        let text = text.trim();
        let body = text
            .strip_prefix("nano_")
            .or_else(|| text.strip_prefix("xrb_"))
            .or_else(|| text.strip_prefix("nano-"))
            .or_else(|| text.strip_prefix("xrb-"));
        let Some(body) = body else {
            bail!("a Nano address starts with nano_");
        };
        ensure!(
            body.len() == 60,
            "a Nano address has 60 characters after nano_"
        );
        let mut bits = Vec::with_capacity(300);
        for c in body.bytes() {
            let Some(digit) = ALPHABET.iter().position(|&a| a == c) else {
                bail!("a Nano address cannot contain `{}`", c as char);
            };
            bits.extend((0..5).rev().map(|shift| (digit >> shift) & 1 == 1));
        }
        ensure!(
            bits[..4].iter().all(|bit| !bit),
            "this is not a Nano address"
        );
        let mut bytes = [0u8; 37];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = bits[4 + index * 8..4 + index * 8 + 8]
                .iter()
                .fold(0u8, |acc, &bit| acc * 2 + u8::from(bit));
        }
        let key: [u8; 32] = bytes[..32].try_into().expect("32 bytes");
        ensure!(
            bytes[32..] == checksum(&key),
            "the address has a typo (its checksum does not match)"
        );
        Ok(Self(key))
    }
}

/// The private key of one account.
pub struct PrivateKey {
    secret: Zeroizing<[u8; 32]>,
    expanded: ExpandedSecretKey,
    public: VerifyingKey,
}

impl PrivateKey {
    pub fn from_bytes(secret: &[u8; 32]) -> Self {
        let wide = Zeroizing::new(<[u8; 64]>::from(Blake2b512::digest(secret)));
        let expanded = ExpandedSecretKey::from_bytes(&wide);
        let public = VerifyingKey::from(&expanded);
        Self {
            secret: Zeroizing::new(*secret),
            expanded,
            public,
        }
    }

    pub fn secret_bytes(&self) -> [u8; 32] {
        *self.secret
    }

    pub fn account(&self) -> Account {
        Account(self.public.to_bytes())
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        raw_sign::<Blake2b512>(&self.expanded, message, &self.public).to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO: &str = "nano_3i1aq1cchnmbn9x5rsbap8b15akfh7wj7pwskuzi7ahz8oq6cobd99d4r3b7";

    #[test]
    fn addresses_go_both_ways() {
        let account: Account = ZERO.parse().unwrap();
        assert_eq!(account.to_string(), ZERO);
        assert_eq!(
            crate::hex(account.as_bytes()),
            "C008B814A7D269A1FA3C6528B19201A24D797912DB9996FF02A1FF356E45552B"
        );
        let old = ZERO.replace("nano_", "xrb_");
        assert_eq!(old.parse::<Account>().unwrap(), account);
        let link = format!("nano:{ZERO}?amount=1000");
        assert_eq!(Account::parse_link(&link).unwrap(), account);
    }

    #[test]
    fn typos_are_caught() {
        let mut typo = ZERO.to_owned();
        typo.replace_range(20..21, "z");
        assert!(typo.parse::<Account>().is_err());
        assert!(ZERO[..40].parse::<Account>().is_err());
        assert!(ZERO.replace("nano_", "btc_").parse::<Account>().is_err());
        assert!(ZERO.replace('3', "0").parse::<Account>().is_err());
    }

    #[test]
    fn signatures_verify() {
        let key = PrivateKey::from_bytes(&[7; 32]);
        let signature = key.sign(b"szept");
        assert!(key.account().verify(b"szept", &signature));
        assert!(!key.account().verify(b"inny", &signature));
    }
}

//! State blocks: the only kind of block the Nano ledger accepts today.

use blake2::{Blake2b, Digest, digest::consts::U32};
use serde_json::{Value, json};

use crate::{Account, PrivateKey, hex};

/// What a block does. Decides the proof-of-work threshold and is passed to
/// the node with the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subtype {
    Send,
    Receive,
    /// The first receive of an account.
    Open,
    Change,
}

impl Subtype {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Receive => "receive",
            Self::Open => "open",
            Self::Change => "change",
        }
    }

    pub fn threshold(self) -> u64 {
        match self {
            Self::Send | Self::Change => crate::work::SEND_THRESHOLD,
            Self::Receive | Self::Open => crate::work::RECEIVE_THRESHOLD,
        }
    }
}

/// A state block: the whole state of an account after one change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateBlock {
    pub account: Account,
    /// The hash of the account's previous block, zero for its first block.
    pub previous: [u8; 32],
    pub representative: Account,
    /// The balance after this block, in raw.
    pub balance: u128,
    /// For a send the recipient's key, for a receive the hash of the send.
    pub link: [u8; 32],
    pub signature: [u8; 64],
    pub work: u64,
}

impl StateBlock {
    /// Blake2b-256 over the preamble (the number 6 in 32 bytes) and the fields.
    pub fn hash(&self) -> [u8; 32] {
        let mut preamble = [0u8; 32];
        preamble[31] = 6;
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(preamble);
        hasher.update(self.account.as_bytes());
        hasher.update(self.previous);
        hasher.update(self.representative.as_bytes());
        hasher.update(self.balance.to_be_bytes());
        hasher.update(self.link);
        hasher.finalize().into()
    }

    /// The root its proof-of-work is computed for: the previous block, or
    /// the account itself for the first block.
    pub fn root(&self) -> [u8; 32] {
        if self.previous == [0; 32] {
            *self.account.as_bytes()
        } else {
            self.previous
        }
    }

    pub fn sign(&mut self, key: &PrivateKey) {
        debug_assert_eq!(key.account(), self.account);
        self.signature = key.sign(&self.hash());
    }

    pub fn verify(&self) -> bool {
        self.account.verify(&self.hash(), &self.signature)
    }

    /// The block as the `process` RPC takes it with `json_block`.
    pub fn to_json(&self) -> Value {
        json!({
            "type": "state",
            "account": self.account.to_string(),
            "previous": hex(&self.previous),
            "representative": self.representative.to_string(),
            "balance": self.balance.to_string(),
            "link": hex(&self.link),
            "signature": hex(&self.signature),
            "work": format!("{:016x}", self.work),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_block_hashes_to_what_its_owner_signed() {
        // The example block of https://docs.nano.org/protocol-design/blocks/.
        // Its signature verifies only if the hash is computed exactly right.
        let block = StateBlock {
            account: "nano_3qgmh14nwztqw4wmcdzy4xpqeejey68chx6nciczwn9abji7ihhum9qtpmdr"
                .parse()
                .unwrap(),
            previous: crate::from_hex(
                "F47B23107E5F34B2CE06F562B5C435DF72A533251CB414C51B2B62A8F63A00E4",
            )
            .unwrap(),
            representative: "nano_1hza3f7wiiqa7ig3jczyxj5yo86yegcmqk3criaz838j91sxcckpfhbhhra1"
                .parse()
                .unwrap(),
            balance: 1_000_000_000_000_000_000_000,
            link: crate::from_hex(
                "19D3D919475DEED4696B5D13018151D1AF88B2BD3BCFF048B45031C1F36D1858",
            )
            .unwrap(),
            signature: crate::from_hex(concat!(
                "3BFBA64A775550E6D49DF1EB8EEC2136DCD74F090E2ED658FBD9E80F17CB1C9F",
                "9F7BDE2B93D95558EC2F277FFF15FD11E6E2162A1714731B743D1E941FA4560A",
            ))
            .unwrap(),
            work: 0xcab7_404f_0b54_49d0,
        };
        assert!(block.verify());
        let link = crate::Account(block.link);
        assert_eq!(
            link.to_string(),
            "nano_18gmu6engqhgtjnppqam181o5nfhj4sdtgyhy36dan3jr9spt84rzwmktafc"
        );
    }

    #[test]
    fn signed_blocks_verify_and_every_field_counts() {
        let key = PrivateKey::from_bytes(&[3; 32]);
        let mut block = StateBlock {
            account: key.account(),
            previous: [1; 32],
            representative: key.account(),
            balance: 42,
            link: [2; 32],
            signature: [0; 64],
            work: 0,
        };
        block.sign(&key);
        assert!(block.verify());
        let hash = block.hash();
        let mut changed = block.clone();
        changed.balance = 41;
        assert_ne!(changed.hash(), hash);
        assert!(!changed.verify());
        changed = block.clone();
        changed.work = 99;
        assert_eq!(changed.hash(), hash, "work is not part of the hash");
        assert_eq!(block.root(), [1; 32]);
        block.previous = [0; 32];
        assert_eq!(block.root(), *key.account().as_bytes());
        let json = block.to_json();
        assert_eq!(json["balance"], "42");
        assert_eq!(json["work"].as_str().unwrap().len(), 16);
    }
}

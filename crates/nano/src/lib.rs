//! A small Nano (XNO) wallet for Szept.
//!
//! The wallet is a standard Nano seed: 32 random bytes, shown to the person
//! as 24 BIP39 words with the seed as entropy, the way Nault and Natrium do
//! it. Account `i` has the private key `blake2b-256(seed || i)` (big-endian
//! `u32`), so the same words open the same accounts in any of those wallets.
//! Szept uses account 0.
//!
//! Everything that touches the ledger goes through the RPC of a Nano node
//! ([`rpc::Rpc`]). Blocks are built and signed here; the node only sees
//! signed blocks, never a key.

pub mod account;
pub mod amount;
pub mod block;
pub mod rpc;
pub mod seed;
pub mod wallet;
pub mod work;

pub use account::{Account, PrivateKey};
pub use amount::{RAW_PER_XNO, format_xno, parse_xno};
pub use block::{StateBlock, Subtype};
pub use rpc::Rpc;
pub use seed::Seed;
pub use wallet::{Balance, Payment, Received, Wallet};

/// Hex in upper case, as the Nano RPC writes hashes and keys.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// Parses `N` bytes of hex, in either case.
pub fn from_hex<const N: usize>(text: &str) -> anyhow::Result<[u8; N]> {
    let text = text.trim();
    anyhow::ensure!(
        text.len() == N * 2 && text.is_ascii(),
        "expected {} hex digits",
        N * 2
    );
    let mut bytes = [0u8; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)?;
    }
    Ok(bytes)
}

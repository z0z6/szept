//! The optional Nano (XNO) wallet.
//!
//! Nothing here runs until the person turns the wallet on. Then a seed is
//! created once, kept in `wallet.seed` next to the identity key, and never
//! overwritten: restoring another backup moves the old seed aside first,
//! and turning the wallet off only hides it. The seed is a standard Nano
//! seed, so its 24 words open the same account in Nault, Natrium or any
//! other Nano wallet, and it goes into every archive.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use komunikator_core::{EndpointId, MessageId, crypto};
use komunikator_nano::{Account, Rpc, Seed, Wallet, format_xno, hex, rpc::DEFAULT_NODES};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::{ChatEvent, EventListener, Shared, save_json};

/// How often waiting payments are received while the app runs.
const RECEIVE_EVERY: Duration = Duration::from_secs(60);

/// How long a payment from a contact is watched until the ledger confirms it.
const CHECK_TRIES: u32 = 24;
const CHECK_EVERY: Duration = Duration::from_secs(5);

/// What `wallet.json` keeps. The seed itself lives in `wallet.seed`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Settings {
    pub enabled: bool,
    /// The person proved they wrote the 24 words down.
    pub backed_up: bool,
    /// Nano nodes to use, in order; empty means the defaults.
    #[serde(default)]
    pub nodes: Vec<String>,
}

pub(crate) struct State {
    pub settings: Settings,
    /// Account 0 of the seed on disk, whether the wallet is on or not.
    pub account: Option<Account>,
    /// The working wallet, while it is on.
    pub active: Option<Arc<Wallet>>,
}

pub(crate) fn seed_path(data_dir: &Path) -> PathBuf {
    data_dir.join("wallet.seed")
}

pub(crate) fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("wallet.json")
}

pub(crate) fn peers_path(data_dir: &Path) -> PathBuf {
    data_dir.join("wallets.json")
}

pub(crate) fn load_settings(data_dir: &Path) -> Settings {
    fs::read(settings_path(data_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub(crate) fn save_settings(data_dir: &Path, settings: &Settings) -> Result<()> {
    save_json(&settings_path(data_dir), settings)
}

pub(crate) fn read_seed(data_dir: &Path) -> Result<Option<Seed>> {
    let path = seed_path(data_dir);
    let bytes = match fs::read(&path) {
        Ok(bytes) => zeroize::Zeroizing::new(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("cannot read the wallet seed"),
    };
    let Ok(bytes) = <[u8; 32]>::try_from(bytes.as_slice()) else {
        bail!("the wallet seed file {} is damaged", path.display());
    };
    Ok(Some(Seed::from_bytes(bytes)))
}

/// Writes `seed` to `wallet.seed`. A different seed already there is moved
/// to `wallet-<time>.seed.old` first, so no seed is ever destroyed.
/// Returns `false` when the same seed was already there.
pub(crate) fn store_seed(data_dir: &Path, seed: &Seed) -> Result<bool> {
    let path = seed_path(data_dir);
    match read_seed(data_dir) {
        Ok(Some(current)) if current == *seed => return Ok(false),
        Ok(Some(_)) | Err(_) if path.exists() => {
            let aside = data_dir.join(format!("wallet-{}.seed.old", now_ms()));
            fs::rename(&path, &aside).context("cannot move the old wallet seed aside")?;
        }
        _ => {}
    }
    fs::create_dir_all(data_dir).context("cannot create the data directory")?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("cannot create {}", path.display()))?;
    file.write_all(seed.bytes())?;
    file.sync_all()?;
    Ok(true)
}

pub(crate) fn rpc(settings: &Settings) -> Result<Rpc> {
    let nodes = if settings.nodes.is_empty() {
        DEFAULT_NODES
            .iter()
            .map(|node| (*node).to_owned())
            .collect()
    } else {
        settings.nodes.clone()
    };
    Rpc::new(nodes, crypto::provider())
}

impl State {
    /// Reads the settings and, when the wallet is on, opens it.
    pub(crate) fn load(data_dir: &Path) -> Result<Self> {
        let settings = load_settings(data_dir);
        let seed = read_seed(data_dir)?;
        let account = seed.as_ref().map(|seed| seed.key(0).account());
        let mut state = Self {
            settings,
            account,
            active: None,
        };
        if let Some(seed) = seed
            && state.settings.enabled
        {
            state.active = Some(Arc::new(Wallet::new(&seed, rpc(&state.settings)?)));
        }
        Ok(state)
    }

    /// Opens the wallet from the seed on disk and turns it on.
    pub(crate) fn open(&mut self, data_dir: &Path) -> Result<()> {
        let seed = read_seed(data_dir)?.context("there is no wallet seed")?;
        self.account = Some(seed.key(0).account());
        self.active = Some(Arc::new(Wallet::new(&seed, rpc(&self.settings)?)));
        self.settings.enabled = true;
        save_settings(data_dir, &self.settings)
    }
}

/// Contacts' Nano accounts, by their Szept identifier.
pub(crate) fn load_peers(data_dir: &Path) -> BTreeMap<String, String> {
    fs::read(peers_path(data_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Receives waiting payments once a minute, and at once when asked.
pub(crate) async fn receive_loop(
    shared: Arc<Shared>,
    listener: Arc<dyn EventListener>,
    kick: Arc<Notify>,
) {
    loop {
        if let Some(wallet) = shared.active_wallet() {
            receive_now(&shared, &wallet, listener.as_ref()).await;
        }
        tokio::select! {
            () = kick.notified() => {}
            () = tokio::time::sleep(RECEIVE_EVERY) => {}
        }
    }
}

/// Receives what waits and reports each payment. Returns what came in.
pub(crate) async fn receive_now(
    shared: &Shared,
    wallet: &Wallet,
    listener: &dyn EventListener,
) -> Vec<crate::ReceivedPayment> {
    let Ok(received) = wallet.receive_all().await else {
        // No node answered; the next round tries again.
        return Vec::new();
    };
    let mut reported = Vec::new();
    for item in received {
        let from = item.source.map(|source| source.to_string());
        let payment = crate::ReceivedPayment {
            block: hex(&item.send),
            amount_raw: item.amount.to_string(),
            amount: format_xno(item.amount),
            peer: from
                .as_deref()
                .and_then(|from| shared.peer_with_wallet(from)),
            from: from.unwrap_or_default(),
        };
        listener.on_event(ChatEvent::NanoReceived {
            payment: payment.clone(),
        });
        reported.push(payment);
    }
    if !reported.is_empty() {
        listener.on_event(ChatEvent::WalletChanged);
    }
    reported
}

/// Watches the payment a contact told us about until the ledger confirms
/// it, and reports what the ledger says: "confirmed", "pending" (sent, not
/// yet confirmed), "invalid" (no such send of that amount to us),
/// "unverified" (no node answered in time) or "no-wallet".
pub(crate) async fn check_payment(
    wallet: Option<Arc<Wallet>>,
    listener: Arc<dyn EventListener>,
    kick: Arc<Notify>,
    peer: EndpointId,
    id: MessageId,
    block: [u8; 32],
    amount: u128,
) {
    let report = |status: &str| {
        listener.on_event(ChatEvent::PaymentChecked {
            peer: peer.to_string(),
            id: id.to_string(),
            status: status.to_owned(),
        });
    };
    let Some(wallet) = wallet else {
        report("no-wallet");
        return;
    };
    let mut told_pending = false;
    for _ in 0..CHECK_TRIES {
        match wallet.check_payment(&block, amount).await {
            Ok(payment) if !payment.matches => {
                report("invalid");
                return;
            }
            Ok(payment) if payment.confirmed => {
                report("confirmed");
                kick.notify_one();
                return;
            }
            Ok(_) if !told_pending => {
                report("pending");
                told_pending = true;
            }
            // Not confirmed yet, or the block has not reached this node.
            _ => {}
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
    report("unverified");
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
    fn a_seed_is_never_destroyed() {
        let dir = std::env::temp_dir().join(format!("szept-wallet-{}", rand::random::<u64>()));
        let first = Seed::generate();
        assert!(store_seed(&dir, &first).unwrap());
        assert!(
            !store_seed(&dir, &first).unwrap(),
            "same seed, nothing to do"
        );
        let second = Seed::generate();
        assert!(store_seed(&dir, &second).unwrap());
        assert_eq!(read_seed(&dir).unwrap().unwrap(), second);
        let old: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".seed.old"))
            .collect();
        assert_eq!(old.len(), 1);
        let kept = fs::read(old[0].path()).unwrap();
        assert_eq!(kept.as_slice(), first.bytes());
        fs::remove_dir_all(dir).unwrap();
    }
}

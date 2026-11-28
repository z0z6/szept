//! The few RPC calls a light wallet needs, against one or more Nano nodes.
//!
//! Nodes are tried in order until one answers; the one that answered is
//! tried first next time. Everything a node says is checked where it can
//! be: work is validated locally, blocks are built and hashed here.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use rustls::crypto::CryptoProvider;
use serde_json::{Value, json};

use crate::{Account, StateBlock, Subtype, from_hex, hex, work};

/// Free public nodes, tried in this order. Anyone can run their own and put
/// it first in the settings.
pub const DEFAULT_NODES: &[&str] = &[
    "https://rpc.nano.to",
    "https://node.somenano.com/proxy",
    "https://rainstorm.city/api",
    "https://nanoslo.0x.no/proxy",
];

/// How long one call may take, work generation included.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The largest answer read from a node.
const MAX_ANSWER: usize = 1024 * 1024;

/// What the ledger knows about an opened account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountInfo {
    pub frontier: [u8; 32],
    pub balance: u128,
    pub representative: Account,
}

/// Money sent to us that we have not received yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub hash: [u8; 32],
    pub amount: u128,
    pub source: Option<Account>,
}

/// What the ledger says about one block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockInfo {
    pub account: Account,
    pub amount: u128,
    pub subtype: String,
    /// For a send: the recipient.
    pub recipient: Option<Account>,
    pub confirmed: bool,
}

#[derive(Clone)]
pub struct Rpc {
    client: reqwest::Client,
    nodes: Arc<Vec<String>>,
    /// The node that answered last.
    preferred: Arc<AtomicUsize>,
}

impl Rpc {
    /// Talks to `nodes` over HTTPS with the given TLS provider (Szept passes
    /// its own, with the post-quantum key exchange first). Plain `http://`
    /// is allowed only for a node on this machine or the local network.
    pub fn new(nodes: Vec<String>, provider: Arc<CryptoProvider>) -> Result<Self> {
        let nodes: Vec<String> = nodes
            .into_iter()
            .map(|node| node.trim().trim_end_matches('/').to_owned())
            .filter(|node| !node.is_empty())
            .collect();
        ensure!(!nodes.is_empty(), "no Nano node is configured");
        for node in &nodes {
            check_node_url(node)?;
        }
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .context("TLS setup failed")?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .user_agent("Szept")
            .build()
            .context("cannot set up the HTTP client")?;
        Ok(Self {
            client,
            nodes: Arc::new(nodes),
            preferred: Arc::default(),
        })
    }

    pub fn nodes(&self) -> &[String] {
        &self.nodes
    }

    /// Sends one call, trying the nodes in turn. An answer with `error` is
    /// a real answer: it is returned as an error without trying the others,
    /// unless the node says it does not offer the call.
    pub async fn call(&self, body: Value) -> Result<Value> {
        let start = self.preferred.load(Ordering::Relaxed);
        let mut failures = Vec::new();
        for step in 0..self.nodes.len() {
            let index = (start + step) % self.nodes.len();
            let node = &self.nodes[index];
            match self.call_one(node, &body).await {
                Ok(answer) => {
                    if let Some(error) = answer.get("error").and_then(Value::as_str) {
                        let lower = error.to_lowercase();
                        let unsupported = lower.contains("not allowed")
                            || lower.contains("unknown command")
                            || lower.contains("disabled")
                            || lower.contains("not supported")
                            || lower.contains("rate limit")
                            || lower.contains("api key")
                            || lower.contains("too many");
                        if unsupported {
                            failures.push(format!("{node}: {error}"));
                            continue;
                        }
                        self.preferred.store(index, Ordering::Relaxed);
                        bail!("{error}");
                    }
                    self.preferred.store(index, Ordering::Relaxed);
                    return Ok(answer);
                }
                Err(err) => failures.push(format!("{node}: {err:#}")),
            }
        }
        Err(anyhow!("no Nano node answered ({})", failures.join("; ")))
    }

    async fn call_one(&self, node: &str, body: &Value) -> Result<Value> {
        let response = self
            .client
            .post(node)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(body)?)
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        ensure!(bytes.len() <= MAX_ANSWER, "the answer is too large");
        let answer: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("not a JSON answer (HTTP {status})"))?;
        ensure!(answer.is_object(), "unexpected answer (HTTP {status})");
        Ok(answer)
    }

    /// The account's state, or `None` while it has never received anything.
    pub async fn account_info(&self, account: &Account) -> Result<Option<AccountInfo>> {
        let answer = self
            .call(json!({
                "action": "account_info",
                "account": account.to_string(),
                "representative": "true",
            }))
            .await;
        let answer = match answer {
            Err(err) if err.to_string().contains("Account not found") => return Ok(None),
            other => other?,
        };
        Ok(Some(AccountInfo {
            frontier: from_hex(text(&answer, "frontier")?)?,
            balance: raw(&answer, "balance")?,
            representative: text(&answer, "representative")?.parse()?,
        }))
    }

    /// Confirmed balance and what is waiting to be received, in raw.
    pub async fn account_balance(&self, account: &Account) -> Result<(u128, u128)> {
        let answer = self
            .call(json!({"action": "account_balance", "account": account.to_string()}))
            .await?;
        let waiting = raw(&answer, "receivable").or_else(|_| raw(&answer, "pending"))?;
        Ok((raw(&answer, "balance")?, waiting))
    }

    /// Up to `count` confirmed sends waiting for the account, largest first.
    pub async fn receivable(&self, account: &Account, count: u32) -> Result<Vec<Pending>> {
        let answer = self
            .call(json!({
                "action": "receivable",
                "account": account.to_string(),
                "count": count.to_string(),
                "source": "true",
                "sorting": "true",
            }))
            .await?;
        parse_receivable(&answer)
    }

    pub async fn block_info(&self, hash: &[u8; 32]) -> Result<BlockInfo> {
        let answer = self
            .call(json!({"action": "block_info", "json_block": "true", "hash": hex(hash)}))
            .await?;
        let contents = answer.get("contents").context("no block in the answer")?;
        let recipient = contents
            .get("link_as_account")
            .and_then(Value::as_str)
            .and_then(|text| text.parse().ok());
        Ok(BlockInfo {
            account: text(&answer, "block_account")?.parse()?,
            amount: raw(&answer, "amount").unwrap_or(0),
            subtype: answer
                .get("subtype")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            recipient,
            confirmed: answer
                .get("confirmed")
                .and_then(Value::as_str)
                .is_some_and(|c| c == "true"),
        })
    }

    /// Publishes a signed block and returns its hash, as the node computed it.
    ///
    /// A block the ledger already has counts as published: when a node
    /// took it but its answer got lost, the next node says "Old block", and
    /// reporting that as a failure would invite paying twice.
    pub async fn process(&self, block: &StateBlock, subtype: Subtype) -> Result<[u8; 32]> {
        ensure!(block.verify(), "the block is not signed");
        let answer = self
            .call(json!({
                "action": "process",
                "json_block": "true",
                "subtype": subtype.as_str(),
                "block": block.to_json(),
            }))
            .await;
        let answer = match answer {
            Err(err) if err.to_string().contains("Old block") => return Ok(block.hash()),
            other => other?,
        };
        let hash: [u8; 32] = from_hex(text(&answer, "hash")?)?;
        ensure!(hash == block.hash(), "the node reported a different block");
        Ok(hash)
    }

    /// Asks a node to compute work, and checks it before trusting it.
    pub async fn work_generate(&self, root: &[u8; 32], threshold: u64) -> Result<u64> {
        let answer = self
            .call(json!({
                "action": "work_generate",
                "hash": hex(root),
                "difficulty": format!("{threshold:016x}"),
            }))
            .await?;
        let work = u64::from_str_radix(text(&answer, "work")?, 16)?;
        ensure!(
            work::is_valid(root, work, threshold),
            "the node sent invalid work"
        );
        Ok(work)
    }

    /// Online representatives and their voting weight in raw.
    pub async fn representatives_online(&self) -> Result<Vec<(Account, u128)>> {
        let answer = self
            .call(json!({"action": "representatives_online", "weight": "true"}))
            .await?;
        let reps = answer
            .get("representatives")
            .and_then(Value::as_object)
            .context("no representatives in the answer")?;
        Ok(reps
            .iter()
            .filter_map(|(address, info)| {
                let account = address.parse().ok()?;
                let weight = info.get("weight")?.as_str()?.parse().ok()?;
                Some((account, weight))
            })
            .collect())
    }
}

fn check_node_url(node: &str) -> Result<()> {
    if node.starts_with("https://") {
        return Ok(());
    }
    let Some(rest) = node.strip_prefix("http://") else {
        bail!("a node address starts with https://");
    };
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    let local = host == "localhost"
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        });
    ensure!(
        local,
        "plain http:// is only allowed for a node on this machine or the local network"
    );
    Ok(())
}

fn text<'a>(answer: &'a Value, key: &str) -> Result<&'a str> {
    answer
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("no `{key}` in the answer"))
}

fn raw(answer: &Value, key: &str) -> Result<u128> {
    text(answer, key)?
        .parse()
        .with_context(|| format!("bad `{key}` in the answer"))
}

/// `blocks` is a map of hash to `{amount, source}`, to a bare amount, or
/// an empty string when nothing waits.
fn parse_receivable(answer: &Value) -> Result<Vec<Pending>> {
    let Some(blocks) = answer.get("blocks").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut pending = Vec::new();
    for (hash, info) in blocks {
        let (amount, source) = match info {
            Value::String(amount) => (amount.as_str(), None),
            Value::Object(fields) => (
                fields.get("amount").and_then(Value::as_str).unwrap_or("0"),
                fields
                    .get("source")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok()),
            ),
            _ => continue,
        };
        let Ok(amount) = amount.parse::<u128>() else {
            continue;
        };
        if amount == 0 {
            continue;
        }
        pending.push(Pending {
            hash: from_hex(hash)?,
            amount,
            source,
        });
    }
    pending.sort_by_key(|item| std::cmp::Reverse(item.amount));
    Ok(pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receivable_answers_in_every_shape_parse() {
        let source = "nano_3i1aq1cchnmbn9x5rsbap8b15akfh7wj7pwskuzi7ahz8oq6cobd99d4r3b7";
        let hash = "000D1BAEC8EC208142C99059B393051BAC8380F9B5A2E6B2489A277D81789F3F";
        let full = json!({"blocks": {hash: {"amount": "6000000000000000000000000000000", "source": source}}});
        let pending = parse_receivable(&full).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].amount, 6 * crate::RAW_PER_XNO);
        assert_eq!(pending[0].source.unwrap().to_string(), source);
        let bare = json!({"blocks": {hash: "5"}});
        assert_eq!(parse_receivable(&bare).unwrap()[0].amount, 5);
        assert!(parse_receivable(&json!({"blocks": ""})).unwrap().is_empty());
    }

    #[test]
    fn only_local_nodes_may_use_plain_http() {
        assert!(check_node_url("https://rpc.nano.to").is_ok());
        assert!(check_node_url("http://127.0.0.1:7076").is_ok());
        assert!(check_node_url("http://192.168.1.5:7076").is_ok());
        assert!(check_node_url("http://localhost:7076").is_ok());
        assert!(check_node_url("http://example.com").is_err());
        assert!(check_node_url("ftp://x").is_err());
    }
}

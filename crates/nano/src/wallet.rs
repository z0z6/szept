//! Account 0 of a seed: balance, sending, receiving, checking payments.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, bail, ensure};

use crate::{
    Account, PrivateKey, Rpc, Seed, StateBlock, Subtype, format_xno,
    rpc::{AccountInfo, Pending},
    work,
};

/// How many waiting sends one round of receiving takes.
const RECEIVE_BATCH: u32 = 20;

/// Balance and what waits to be received, both in raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Balance {
    pub balance: u128,
    pub receivable: u128,
}

/// A send we just received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The hash of the send block, the one a sender reports in a chat.
    pub send: [u8; 32],
    pub amount: u128,
    pub source: Option<Account>,
}

/// What the ledger says about a payment someone told us about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    /// The block is a send of exactly the reported amount to us.
    pub matches: bool,
    pub confirmed: bool,
    /// The account that sent it.
    pub from: Account,
    pub amount: u128,
}

/// Work found ahead of time: the root it is for and the nonce.
type ReadyWork = Arc<Mutex<Option<([u8; 32], u64)>>>;

/// Sets the flag when dropped, so an abandoned search stops.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub struct Wallet {
    key: PrivateKey,
    account: Account,
    rpc: Rpc,
    /// Only one block at a time: each one builds on the previous.
    chain: tokio::sync::Mutex<()>,
    /// Work computed ahead for the next block: its root and the nonce.
    ready: ReadyWork,
    /// Stops the search for work ahead when a newer one starts.
    ahead: Mutex<Option<Arc<AtomicBool>>>,
    /// Ask nodes for work as well as computing it here.
    remote_work: bool,
}

impl Wallet {
    /// Account 0 of `seed`, the one every Nano wallet opens first.
    pub fn new(seed: &Seed, rpc: Rpc) -> Self {
        let key = seed.key(0);
        Self {
            account: key.account(),
            key,
            rpc,
            chain: tokio::sync::Mutex::new(()),
            ready: Arc::default(),
            ahead: Mutex::default(),
            remote_work: true,
        }
    }

    /// Computes all work on this device, never asking a node.
    pub fn local_work_only(mut self) -> Self {
        self.remote_work = false;
        self
    }

    pub fn account(&self) -> Account {
        self.account
    }

    pub fn rpc(&self) -> &Rpc {
        &self.rpc
    }

    pub async fn balance(&self) -> Result<Balance> {
        let (balance, receivable) = self.rpc.account_balance(&self.account).await?;
        Ok(Balance {
            balance,
            receivable,
        })
    }

    /// Sends `amount` raw to `to` and returns the hash of the send block.
    /// Computing the work can take a while on a slow device.
    pub async fn send(&self, to: Account, amount: u128) -> Result<[u8; 32]> {
        ensure!(amount > 0, "the amount must be more than zero");
        ensure!(to != self.account, "this is the wallet's own address");
        let _chain = self.chain.lock().await;
        let info = self
            .rpc
            .account_info(&self.account)
            .await?
            .context("the wallet is empty: nothing has been received yet")?;
        ensure!(
            info.balance >= amount,
            "not enough XNO: the balance is {}",
            format_xno(info.balance)
        );
        let mut block = StateBlock {
            account: self.account,
            previous: info.frontier,
            representative: info.representative,
            balance: info.balance - amount,
            link: to.0,
            signature: [0; 64],
            work: 0,
        };
        block.work = self.work(block.root(), Subtype::Send).await?;
        block.sign(&self.key);
        let hash = self.rpc.process(&block, Subtype::Send).await?;
        self.work_ahead(hash);
        Ok(hash)
    }

    /// Receives what waits for us, largest first, and returns what came in.
    pub async fn receive_all(&self) -> Result<Vec<Received>> {
        let _chain = self.chain.lock().await;
        let pending = self.rpc.receivable(&self.account, RECEIVE_BATCH).await?;
        if pending.is_empty() {
            return Ok(Vec::new());
        }
        let mut info = self.rpc.account_info(&self.account).await?;
        let mut received = Vec::new();
        for item in pending {
            match self.receive_one(info.as_ref(), &item).await {
                Ok(next) => {
                    info = Some(next);
                    received.push(Received {
                        send: item.hash,
                        amount: item.amount,
                        source: item.source,
                    });
                }
                // What was received so far stays; the rest waits for the next round.
                Err(err) if received.is_empty() => return Err(err),
                Err(_) => break,
            }
        }
        if let Some(info) = &info {
            self.work_ahead(info.frontier);
        }
        Ok(received)
    }

    async fn receive_one(&self, info: Option<&AccountInfo>, item: &Pending) -> Result<AccountInfo> {
        let (previous, balance, representative, subtype) = match info {
            Some(info) => (
                info.frontier,
                info.balance,
                info.representative,
                Subtype::Receive,
            ),
            None => (
                [0; 32],
                0,
                self.choose_representative(item.source).await?,
                Subtype::Open,
            ),
        };
        let mut block = StateBlock {
            account: self.account,
            previous,
            representative,
            balance: balance
                .checked_add(item.amount)
                .context("the balance would overflow")?,
            link: item.hash,
            signature: [0; 64],
            work: 0,
        };
        block.work = self.work(block.root(), subtype).await?;
        block.sign(&self.key);
        let hash = self.rpc.process(&block, subtype).await?;
        Ok(AccountInfo {
            frontier: hash,
            balance: block.balance,
            representative,
        })
    }

    /// Checks that `block` sends exactly `amount` raw to us.
    pub async fn check_payment(&self, block: &[u8; 32], amount: u128) -> Result<Payment> {
        let info = self.rpc.block_info(block).await?;
        Ok(Payment {
            matches: info.subtype == "send"
                && info.recipient == Some(self.account)
                && info.amount == amount,
            confirmed: info.confirmed,
            from: info.account,
            amount: info.amount,
        })
    }

    /// A representative for a new account: a random online one with a real
    /// share of the vote but not one of the giants, which spreads the weight
    /// the way the Nano docs ask. Falls back to the sender's representative.
    async fn choose_representative(&self, source: Option<Account>) -> Result<Account> {
        if let Ok(reps) = self.rpc.representatives_online().await {
            let total: u128 = reps.iter().map(|(_, weight)| *weight).sum();
            let mut candidates: Vec<Account> = reps
                .iter()
                .filter(|(_, weight)| *weight >= total / 1000 && *weight <= total / 100 * 3)
                .map(|(account, _)| *account)
                .collect();
            // A small network, or a node that lists only a few: any online one.
            if candidates.is_empty() {
                candidates = reps.iter().map(|(account, _)| *account).collect();
            }
            if !candidates.is_empty() {
                let index = (rand::random::<u64>() % candidates.len() as u64) as usize;
                return Ok(candidates[index]);
            }
        }
        if let Some(source) = source
            && let Ok(Some(info)) = self.rpc.account_info(&source).await
        {
            return Ok(info.representative);
        }
        bail!("cannot pick a representative: no Nano node answered with one")
    }

    /// Work for `root`: computed ahead, from a node, or here, whichever
    /// comes first. Every result is checked.
    async fn work(&self, root: [u8; 32], subtype: Subtype) -> Result<u64> {
        let threshold = subtype.threshold();
        if let Some((ready_root, nonce)) = *self.ready.lock().unwrap()
            && ready_root == root
            && work::is_valid(&root, nonce, threshold)
        {
            return Ok(nonce);
        }
        find_work(&self.rpc, root, threshold, self.remote_work).await
    }

    /// Starts computing the next send's work in the background, so the next
    /// payment leaves at once.
    fn work_ahead(&self, root: [u8; 32]) {
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(old) = self.ahead.lock().unwrap().replace(stop.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        let rpc = self.rpc.clone();
        let ready = self.ready.clone();
        let remote = self.remote_work;
        tokio::spawn(async move {
            let search = find_work(&rpc, root, work::SEND_THRESHOLD, remote);
            tokio::pin!(search);
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                tokio::select! {
                    found = &mut search => {
                        if let Ok(nonce) = found {
                            *ready.lock().unwrap() = Some((root, nonce));
                        }
                        return;
                    }
                    () = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
                }
            }
        });
    }
}

impl Drop for Wallet {
    fn drop(&mut self) {
        if let Some(ahead) = self.ahead.lock().unwrap().take() {
            ahead.store(true, Ordering::Relaxed);
        }
    }
}

/// Races the local search against a node; the first valid answer wins and
/// the local search stops.
async fn find_work(rpc: &Rpc, root: [u8; 32], threshold: u64, remote: bool) -> Result<u64> {
    let stop = Arc::new(AtomicBool::new(false));
    let _stop = StopOnDrop(stop.clone());
    let flag = stop.clone();
    let mut local = tokio::task::spawn_blocking(move || work::generate(root, threshold, &flag));
    let from_node = rpc.work_generate(&root, threshold);
    tokio::pin!(from_node);
    let mut asked = !remote;
    loop {
        tokio::select! {
            found = &mut local => {
                return found
                    .context("the work search failed")?
                    .context("the work search was stopped");
            }
            answer = &mut from_node, if !asked => {
                match answer {
                    Ok(nonce) => return Ok(nonce),
                    Err(_) => asked = true,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Whole payments against a small fake node that keeps a real ledger:
    //! it checks every block's signature, work, previous block and balance.

    use std::collections::HashMap;

    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;
    use crate::{RAW_PER_XNO, from_hex, hex};

    #[derive(Default)]
    struct Ledger {
        /// Account → (frontier, balance, representative).
        accounts: HashMap<Account, ([u8; 32], u128, Account)>,
        /// Send hash → (from, to, amount); removed once received.
        pending: HashMap<[u8; 32], (Account, Account, u128)>,
        sends: HashMap<[u8; 32], (Account, Account, u128)>,
        processed: u32,
    }

    fn block_from(json: &Value) -> StateBlock {
        let text = |key: &str| json[key].as_str().unwrap().to_owned();
        StateBlock {
            account: text("account").parse().unwrap(),
            previous: from_hex(&text("previous")).unwrap(),
            representative: text("representative").parse().unwrap(),
            balance: text("balance").parse().unwrap(),
            link: from_hex(&text("link")).unwrap(),
            signature: from_hex(&text("signature")).unwrap(),
            work: u64::from_str_radix(&text("work"), 16).unwrap(),
        }
    }

    fn answer(ledger: &mut Ledger, request: &Value) -> Value {
        let account = || -> Account { request["account"].as_str().unwrap().parse().unwrap() };
        match request["action"].as_str().unwrap() {
            "account_info" => match ledger.accounts.get(&account()) {
                Some((frontier, balance, rep)) => json!({
                    "frontier": hex(frontier),
                    "balance": balance.to_string(),
                    "representative": rep.to_string(),
                }),
                None => json!({"error": "Account not found"}),
            },
            "account_balance" => {
                let me = account();
                let balance = ledger.accounts.get(&me).map_or(0, |a| a.1);
                let waiting: u128 = ledger
                    .pending
                    .values()
                    .filter(|p| p.1 == me)
                    .map(|p| p.2)
                    .sum();
                json!({"balance": balance.to_string(), "receivable": waiting.to_string()})
            }
            "receivable" => {
                let me = account();
                let blocks: serde_json::Map<String, Value> = ledger
                    .pending
                    .iter()
                    .filter(|(_, p)| p.1 == me)
                    .map(|(hash, p)| {
                        (
                            hex(hash),
                            json!({"amount": p.2.to_string(), "source": p.0.to_string()}),
                        )
                    })
                    .collect();
                if blocks.is_empty() {
                    json!({"blocks": ""})
                } else {
                    json!({"blocks": blocks})
                }
            }
            "representatives_online" => json!({"representatives": {
                "nano_3i1aq1cchnmbn9x5rsbap8b15akfh7wj7pwskuzi7ahz8oq6cobd99d4r3b7": {"weight": "100"}
            }}),
            // Like most public nodes: no work from here.
            "work_generate" => json!({"error": "Action not allowed"}),
            "block_info" => {
                let hash: [u8; 32] = from_hex(request["hash"].as_str().unwrap()).unwrap();
                match ledger.sends.get(&hash) {
                    Some((from, to, amount)) => json!({
                        "block_account": from.to_string(),
                        "amount": amount.to_string(),
                        "subtype": "send",
                        "confirmed": "true",
                        "contents": {"link_as_account": to.to_string()},
                    }),
                    None => json!({"error": "Block not found"}),
                }
            }
            "process" => {
                let block = block_from(&request["block"]);
                let subtype = request["subtype"].as_str().unwrap();
                let hash = block.hash();
                if !block.verify() {
                    return json!({"error": "Bad signature"});
                }
                let threshold = if subtype == "send" {
                    work::SEND_THRESHOLD
                } else {
                    work::RECEIVE_THRESHOLD
                };
                if !work::is_valid(&block.root(), block.work, threshold) {
                    return json!({"error": "Block work is insufficient"});
                }
                let (frontier, balance) = ledger
                    .accounts
                    .get(&block.account)
                    .map_or(([0; 32], 0), |a| (a.0, a.1));
                if block.previous != frontier {
                    return json!({"error": "Fork"});
                }
                match subtype {
                    "send" => {
                        assert!(block.balance < balance);
                        let to = Account(block.link);
                        let entry = (block.account, to, balance - block.balance);
                        ledger.pending.insert(hash, entry);
                        ledger.sends.insert(hash, entry);
                    }
                    "receive" | "open" => {
                        let (_, to, amount) = ledger
                            .pending
                            .remove(&block.link)
                            .expect("nothing to receive");
                        assert_eq!(to, block.account);
                        assert_eq!(block.balance, balance + amount);
                        assert_eq!(subtype == "open", frontier == [0; 32]);
                    }
                    other => panic!("unexpected subtype {other}"),
                }
                ledger
                    .accounts
                    .insert(block.account, (hash, block.balance, block.representative));
                ledger.processed += 1;
                json!({"hash": hex(&hash)})
            }
            other => panic!("unexpected call {other}"),
        }
    }

    /// Serves plain HTTP on localhost, one request per connection.
    async fn fake_node(ledger: Arc<Mutex<Ledger>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let ledger = ledger.clone();
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let body = loop {
                        let read = stream.read(&mut chunk).await.unwrap();
                        if read == 0 {
                            return;
                        }
                        buffer.extend_from_slice(&chunk[..read]);
                        let text = String::from_utf8_lossy(&buffer).into_owned();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let length: usize = text[..end]
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    name.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse().ok())?
                                })
                                .unwrap_or(0);
                            if buffer.len() >= end + 4 + length {
                                break buffer[end + 4..end + 4 + length].to_vec();
                            }
                        }
                    };
                    let request: Value = serde_json::from_slice(&body).unwrap();
                    let reply = answer(&mut ledger.lock().unwrap(), &request).to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        address
    }

    fn rpc(node: &str) -> Rpc {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        // The first node is down: calls must fail over to the second.
        Rpc::new(vec!["http://127.0.0.1:9".into(), node.to_owned()], provider).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn money_goes_round_and_lands_where_it_should() {
        let ledger = Arc::new(Mutex::new(Ledger::default()));
        let node = fake_node(ledger.clone()).await;
        let ala = Wallet::new(&Seed::generate(), rpc(&node));
        let ola = Wallet::new(&Seed::generate(), rpc(&node));

        // A faucet pays Ala 10 XNO.
        let faucet = Seed::generate().key(0);
        let faucet_account = faucet.account();
        let gift: [u8; 32] = [42; 32];
        ledger
            .lock()
            .unwrap()
            .pending
            .insert(gift, (faucet_account, ala.account(), 10 * RAW_PER_XNO));

        assert!(
            ala.send(ola.account(), 1).await.is_err(),
            "nothing received yet"
        );
        assert_eq!(ala.balance().await.unwrap().receivable, 10 * RAW_PER_XNO);
        let received = ala.receive_all().await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].source, Some(faucet_account));
        assert_eq!(ala.balance().await.unwrap().balance, 10 * RAW_PER_XNO);

        // Ala pays Ola twice; the second one builds on the first.
        let half = RAW_PER_XNO / 2;
        let first = ala.send(ola.account(), 3 * RAW_PER_XNO).await.unwrap();
        let second = ala.send(ola.account(), half).await.unwrap();
        assert!(
            ala.send(ola.account(), 100 * RAW_PER_XNO).await.is_err(),
            "not enough"
        );
        assert!(ala.send(ala.account(), 1).await.is_err(), "to itself");

        // Ola checks what Ala told her, and only that.
        let check = ola.check_payment(&first, 3 * RAW_PER_XNO).await.unwrap();
        assert!(check.matches && check.confirmed);
        assert_eq!(check.from, ala.account());
        let lie = ola.check_payment(&second, 5 * RAW_PER_XNO).await.unwrap();
        assert!(!lie.matches, "a bigger amount than sent");
        assert!(
            !ala.check_payment(&first, 3 * RAW_PER_XNO)
                .await
                .unwrap()
                .matches
        );

        let received = ola.receive_all().await.unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].amount, 3 * RAW_PER_XNO, "largest first");
        let ola_balance = ola.balance().await.unwrap();
        assert_eq!(
            (ola_balance.balance, ola_balance.receivable),
            (3 * RAW_PER_XNO + half, 0)
        );
        assert_eq!(
            ala.balance().await.unwrap().balance,
            10 * RAW_PER_XNO - 3 * RAW_PER_XNO - half
        );
        assert!(ola.receive_all().await.unwrap().is_empty());
        assert_eq!(ledger.lock().unwrap().processed, 5);
    }
}

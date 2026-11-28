//! Proof-of-work: a nonce whose Blake2b-64 with the block's root clears a
//! threshold. It is Nano's spam protection instead of a fee.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use blake2::{Blake2b, Digest, digest::consts::U8};

/// Sends and representative changes (epoch 2): about 2^29 tries on average.
#[cfg(not(test))]
pub const SEND_THRESHOLD: u64 = 0xffff_fff8_0000_0000;

/// Receives (epoch 2): 64 times easier, about 2^23 tries.
#[cfg(not(test))]
pub const RECEIVE_THRESHOLD: u64 = 0xffff_fe00_0000_0000;

// Unit tests run whole payments against a fake node: real thresholds would
// make each one take minutes.
#[cfg(test)]
pub const SEND_THRESHOLD: u64 = 0xfff0_0000_0000_0000;
#[cfg(test)]
pub const RECEIVE_THRESHOLD: u64 = 0xff00_0000_0000_0000;

/// How well `work` does for `root`; valid when at least the threshold.
pub fn value(root: &[u8; 32], work: u64) -> u64 {
    let mut hasher = Blake2b::<U8>::new();
    hasher.update(work.to_le_bytes());
    hasher.update(root);
    u64::from_le_bytes(hasher.finalize().into())
}

pub fn is_valid(root: &[u8; 32], work: u64, threshold: u64) -> bool {
    value(root, work) >= threshold
}

/// Searches on every core until it finds work or `stop` is set. Blocks the
/// calling thread: run it with `spawn_blocking`.
pub fn generate(root: [u8; 32], threshold: u64, stop: &Arc<AtomicBool>) -> Option<u64> {
    let threads = thread::available_parallelism().map_or(2, |n| n.get());
    let found = Arc::new(std::sync::Mutex::new(None));
    thread::scope(|scope| {
        for _ in 0..threads {
            let found = found.clone();
            let stop = stop.clone();
            scope.spawn(move || {
                let mut nonce: u64 = rand::random();
                // Check the stop flag every so often, not on every try.
                while !stop.load(Ordering::Relaxed) {
                    for _ in 0..4096 {
                        if value(&root, nonce) >= threshold {
                            *found.lock().unwrap() = Some(nonce);
                            stop.store(true, Ordering::Relaxed);
                            return;
                        }
                        nonce = nonce.wrapping_add(1);
                    }
                }
            });
        }
    });
    *found.lock().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_work_is_valid() {
        let root = [5u8; 32];
        // An easy threshold keeps the test fast.
        let easy = 0xff00_0000_0000_0000;
        let work = generate(root, easy, &Arc::new(AtomicBool::new(false))).unwrap();
        assert!(is_valid(&root, work, easy));
        assert!(!is_valid(&[6; 32], work, u64::MAX));
    }

    #[test]
    fn the_documented_work_has_the_documented_difficulty() {
        // The `work_generate` example of the Nano RPC documentation.
        let root: [u8; 32] =
            crate::from_hex("718CC2121C3E641059BC1C2CFC45666C99E8AE922F7A807B7D07B62C995D79E2")
                .unwrap();
        assert_eq!(value(&root, 0x2b3d_689b_bcb2_1dca), 0xffff_fff9_3c41_ec94);
    }

    #[test]
    fn a_stopped_search_gives_up() {
        let stop = Arc::new(AtomicBool::new(true));
        assert_eq!(generate([0; 32], u64::MAX, &stop), None);
    }
}

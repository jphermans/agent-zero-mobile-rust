//! Small deterministic helpers: seeded retry jitter and UUID-shaped identifiers.
//! No external rng/uuid dependencies (task constraint); all values derive from
//! caller-supplied seeds or system entropy through SplitMix64.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Deterministic [0,1) generator seeded from a caller-supplied `u64` (task:
/// "seeded from a passed u64", no randomness dependency).
pub(crate) struct SeededJitter(u64);

impl SeededJitter {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed ^ 0xa0a0_a0a0_a0a0_a0a1) // never zero
    }

    /// SplitMix64 step, returns a value in [0, 1).
    pub(crate) fn next_unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Backoff for attempt `attempt` (0-based) from the constant base table,
    /// scaled by `1 + jitter * 0.25` per §6 (`PollRecovery.swift:12-19`):
    /// delays 2^attempts seconds (1, 2, 4, 8).
    pub(crate) fn backoff(&mut self, attempt: usize, table: &[Duration; 4]) -> Duration {
        let jitter = self.next_unit();
        let base = table[attempt.min(3)];
        base.mul_f64(1.0 + jitter * 0.25)
    }
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// UUID-v4-shaped identifier built from wall-clock nanos + pid + a process
/// counter (used for message ids, upload names and multipart boundaries).
pub(crate) fn new_uuid_v4() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut state = nanos ^ pid.rotate_left(32) ^ counter.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let a = splitmix64(&mut state);
    let b = splitmix64(&mut state);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&a.to_be_bytes());
    bytes[8..].copy_from_slice(&b.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]
    )
}

/// RFC 3986 unreserved-character percent-encoding, used for the
/// `/api/image_get?path=...` query value (§3.9).
pub(crate) fn query_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

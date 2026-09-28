//! Bytes carried by every RPC connection of the process, both ways, for a live throughput.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static SENT: AtomicU64 = AtomicU64::new(0);
static RECEIVED: AtomicU64 = AtomicU64::new(0);

pub(crate) fn sent(bytes: usize) {
    SENT.fetch_add(bytes as u64, Relaxed);
}

pub(crate) fn received(bytes: usize) {
    RECEIVED.fetch_add(bytes as u64, Relaxed);
}

/// Bytes sent and received so far, by all connections.
pub fn traffic() -> (u64, u64) {
    (SENT.load(Relaxed), RECEIVED.load(Relaxed))
}

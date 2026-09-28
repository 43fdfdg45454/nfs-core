//! Memory for pieces, shared by every open file: a total the files split among themselves (what
//! does not fit stays on disk), and how much is in use now and at most.

use crate::Config;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

#[derive(Default)]
pub struct Memory {
    used: AtomicU64,
    peak: AtomicU64,
    /// Files open for reading.
    pub(crate) files: AtomicUsize,
}

impl Memory {
    pub fn add(&self, bytes: u64) {
        let used = self.used.fetch_add(bytes, Relaxed) + bytes;
        self.peak.fetch_max(used, Relaxed);
    }

    pub fn sub(&self, bytes: u64) {
        self.used.fetch_sub(bytes, Relaxed);
    }

    /// Bytes in memory now, and the most there were.
    pub fn used(&self) -> (u64, u64) {
        (self.used.load(Relaxed), self.peak.load(Relaxed))
    }

    /// Each file's memory ahead of and behind its readers: at most the configured ones, and all
    /// of them together within the total (two thirds ahead, one third behind).
    pub fn shares(&self, config: &Config) -> (u64, u64) {
        let files = self.files.load(Relaxed).max(1) as u64;
        let ahead = config.memory_ahead.min(config.memory_total * 2 / 3 / files);
        (ahead, config.memory_behind.min(config.memory_total / 3 / files))
    }
}

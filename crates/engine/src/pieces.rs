//! A file's pieces in memory, and those being fetched: whoever needs one of those waits for its
//! fetch instead of asking again.

use crate::memory::Memory;
use bytes::Bytes;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use tokio::sync::watch;

pub struct Pieces {
    ready: HashMap<u64, Bytes>,
    pending: HashMap<u64, watch::Receiver<bool>>,
    /// Counts the bytes kept, with every other file's.
    memory: Arc<Memory>,
}

impl Pieces {
    pub fn new(memory: Arc<Memory>) -> Self {
        Self { ready: HashMap::new(), pending: HashMap::new(), memory }
    }

    pub fn get(&self, i: u64) -> Option<Bytes> {
        self.ready.get(&i).cloned()
    }

    /// The fetch under way for piece `i`, to wait for.
    pub fn pending(&self, i: u64) -> Option<watch::Receiver<bool>> {
        self.pending.get(&i).cloned()
    }

    pub fn has(&self, i: u64) -> bool {
        self.ready.contains_key(&i) || self.pending.contains_key(&i)
    }

    /// Marks as being fetched (by the fetch behind `done`) the pieces of `range` neither here nor
    /// under way, and returns them.
    pub fn claim(&mut self, range: Range<u64>, done: &watch::Sender<bool>) -> Vec<u64> {
        let claimed: Vec<u64> = range.filter(|i| !self.has(*i)).collect();
        claimed.iter().for_each(|i| _ = self.pending.insert(*i, done.subscribe()));
        claimed
    }

    /// A fetched piece; kept in memory only if `keep` (near a reader), else only on disk.
    pub fn put(&mut self, i: u64, data: Bytes, keep: bool) {
        self.pending.remove(&i);
        if keep {
            self.memory.add(data.len() as u64);
            if let Some(old) = self.ready.insert(i, data) {
                self.memory.sub(old.len() as u64);
            }
        }
    }

    pub fn fail(&mut self, i: u64) {
        self.pending.remove(&i);
    }

    /// Drops from memory the pieces `keep` says no reader is near.
    pub fn trim(&mut self, keep: impl Fn(u64) -> bool) {
        let memory = &self.memory;
        self.ready.retain(|i, data| keep(*i) || (memory.sub(data.len() as u64), false).1);
    }
}

impl Drop for Pieces {
    fn drop(&mut self) {
        self.memory.sub(self.ready.values().map(|d| d.len() as u64).sum());
    }
}

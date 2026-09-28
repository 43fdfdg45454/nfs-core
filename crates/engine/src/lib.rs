//! The file engine the app's file descriptors go through. Reads are pieces of 128 KiB: what a
//! reader waits for is fetched at once on the session's priority lane; what it will read next is
//! fetched ahead in 1 MiB blocks through a queue shared by all files, nearest first. Pieces live
//! in memory near the readers and on disk (the cache) for later. Writes go out in parallel.

mod ahead;
mod disk;
mod fetch;
mod memory;
mod names;
mod pieces;
mod queue;
mod reader;
mod shared;
mod writer;

pub use disk::DiskCache;
pub use reader::Reader;
pub use writer::Writer;

use nfs_client::{Client, Fh, Result};
use queue::Queue;
use shared::Shared;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

/// Where a read's data came from, from fastest to slowest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Memory,
    Disk,
    /// The reader waited for the server.
    Network,
}

/// The unit of reading and of caching.
pub const PIECE: u64 = 128 << 10;
/// Pieces per block: a block is one READ of 1 MiB.
pub const BLOCK: u64 = 8;

#[derive(Clone)]
pub struct Config {
    /// Read-ahead READs (and WRITEs) in flight, for all files together; 0: the larger of 8 and the
    /// session's connections (over TCP with loss, each connection must be busy to fill the link).
    pub in_flight: usize,
    /// How far ahead a steady reader is read, in bytes (memory and disk).
    pub read_ahead: u64,
    /// Of that, how much is kept in memory; the rest only on disk.
    pub memory_ahead: u64,
    /// Memory kept behind each reader, for going back a little.
    pub memory_behind: u64,
    /// Memory for all open files together: with several, each gets a share of it (the rest of
    /// their read-ahead stays on disk).
    pub memory_total: u64,
    pub cache: Option<Arc<DiskCache>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            in_flight: 8,
            read_ahead: 256 << 20,
            memory_ahead: 48 << 20,
            memory_behind: 24 << 20,
            memory_total: 96 << 20,
            cache: None,
        }
    }
}

pub struct Engine {
    client: Arc<Client>,
    config: Config,
    queue: Arc<Queue>,
    memory: Arc<memory::Memory>,
    /// Files open for reading, by handle, while some reader has them.
    open: Mutex<HashMap<Fh, Weak<Shared>>>,
}

impl Engine {
    pub fn new(client: Arc<Client>, mut config: Config) -> Arc<Self> {
        if config.in_flight == 0 {
            config.in_flight = client.connections().max(8);
        }
        let queue = Queue::start(config.in_flight);
        let memory = Arc::default();
        Arc::new(Self { client, config, queue, memory, open: Mutex::default() })
    }

    pub fn client(&self) -> &Arc<Client> {
        &self.client
    }

    /// A reader of `fh`. Readers of the same version of a file share its pieces; a file changed
    /// on the server since (another `change` attribute) is opened anew. While the read delegation
    /// that version was opened with is held, nobody changed it: no need to ask.
    pub async fn read(self: &Arc<Self>, fh: &Fh) -> Result<Reader> {
        let found = self.open.lock().expect("not poisoned").get(fh).and_then(Weak::upgrade);
        let delegation = self.client.delegation(fh);
        if let Some(shared) = found.as_ref().filter(|s| delegation.is_some_and(|d| s.delegated(d)))
        {
            return Ok(Reader::new(shared.clone()));
        }
        let attrs = self.client.getattr(fh).await?;
        let shared = match found.filter(|s| s.version == attrs.change) {
            Some(shared) => shared,
            None => {
                let shared = Shared::open(self, fh, &attrs).await?;
                self.open.lock().expect("not poisoned").insert(fh.clone(), Arc::downgrade(&shared));
                shared
            }
        };
        Ok(Reader::new(shared))
    }
}

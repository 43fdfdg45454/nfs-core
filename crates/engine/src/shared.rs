//! One version of a file open for reading, shared by its readers: its pieces, its readers'
//! positions (what is worth keeping and fetching), and the fetches themselves.

use crate::disk::DiskFile;
use crate::pieces::Pieces;
use crate::queue::Queue;
use crate::{BLOCK, Config, Engine, PIECE};
use nfs_client::attr::Attrs;
use nfs_client::{Fh, File, READ_ACCESS, Result};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

pub struct Shared {
    pub version: u64,
    pub size: u64,
    pub(crate) file: Arc<File>,
    pub(crate) config: Config,
    pub(crate) queue: Arc<Queue>,
    pub(crate) pieces: Mutex<Pieces>,
    /// Each reader's position and how far it read without jumping.
    readers: Mutex<HashMap<u64, (u64, u64)>>,
    /// Blocks in the read-ahead queue.
    pub(crate) queued: Mutex<HashSet<u64>>,
    pub(crate) disk: Option<DiskFile>,
    next_reader: AtomicU64,
    /// Removed or renamed through the engine: nothing more is cached for it.
    forgotten: std::sync::atomic::AtomicBool,
    /// Where the file ends: its size, or less once a READ found it truncated.
    end: AtomicU64,
    /// The memory all open files share (this one counts as open while it lives).
    memory: Arc<crate::memory::Memory>,
}

impl Shared {
    pub async fn open(engine: &Engine, fh: &Fh, attrs: &Attrs) -> Result<Arc<Self>> {
        let file = Arc::new(engine.client.open(fh, READ_ACCESS).await?);
        let disk =
            engine.config.cache.as_ref().and_then(|cache| cache.file(fh, attrs.change, attrs.size));
        let (config, queue) = (engine.config.clone(), engine.queue.clone());
        let (readers, queued) = Default::default();
        let pieces = Mutex::new(Pieces::new(engine.memory.clone()));
        let memory = engine.memory.clone();
        memory.files.fetch_add(1, Relaxed);
        Ok(Arc::new(Self {
            version: attrs.change,
            size: attrs.size,
            file,
            config,
            queue,
            pieces,
            readers,
            queued,
            disk,
            next_reader: 0.into(),
            forgotten: false.into(),
            end: attrs.size.into(),
            memory,
        }))
    }

    /// Whether this version was opened with delegation `d`.
    pub fn delegated(&self, d: nfs_client::Stateid) -> bool {
        self.file.delegation().is_some_and(|mine| mine.other == d.other)
    }

    pub fn forget(&self) {
        self.forgotten.store(true, Relaxed);
    }

    pub fn forgotten(&self) -> bool {
        self.forgotten.load(Relaxed)
    }

    pub fn end(&self) -> u64 {
        self.end.load(Relaxed)
    }

    /// Someone else truncated the file: nothing past `end` is read any more.
    pub(crate) fn shrink(&self, end: u64) {
        self.end.fetch_min(end, Relaxed);
    }

    pub fn add_reader(&self) -> u64 {
        let id = self.next_reader.fetch_add(1, Relaxed);
        self.readers.lock().expect("not poisoned").insert(id, (0, 0));
        id
    }

    pub fn set_reader(&self, id: u64, position: u64, run: u64) {
        self.readers.lock().expect("not poisoned").insert(id, (position, run));
    }

    pub fn remove_reader(&self, id: u64) {
        self.readers.lock().expect("not poisoned").remove(&id);
    }

    /// Whether some reader's window (what it will read soon) holds `offset`.
    fn near(&self, offset: u64, window: impl Fn(u64) -> (u64, u64)) -> bool {
        let readers = self.readers.lock().expect("not poisoned");
        readers.values().any(|&(pos, run)| {
            let (behind, ahead) = window(run);
            offset + behind >= pos && offset <= pos + ahead
        })
    }

    pub(crate) fn in_memory(&self, i: u64) -> bool {
        let c = &self.config;
        self.near(i * PIECE, |run| {
            let (ahead, behind) = self.memory.shares(c);
            (behind, crate::ahead::window(run, c).min(ahead))
        })
    }

    pub fn wanted(&self, block: u64) -> bool {
        self.near(block * BLOCK * PIECE, |run| (0, crate::ahead::window(run, &self.config)))
    }

    /// Whether the piece is in memory, under way, or on disk.
    pub fn present(&self, i: u64) -> bool {
        self.pieces.lock().expect("not poisoned").has(i)
            || self.disk.as_ref().is_some_and(|d| d.has(i))
    }
}

/// The last reader is gone: the file is closed on the server, in the background.
impl Drop for Shared {
    fn drop(&mut self) {
        self.memory.files.fetch_sub(1, Relaxed);
        let file = self.file.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move { _ = file.close().await });
        }
    }
}

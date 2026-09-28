//! The disk cache: one sparse file per version of a remote file, plus an index of the pieces it
//! holds. It stays under its size limit and leaves the file system `MIN_FREE`: the least recently
//! opened files go first. A new version of a file drops the old ones.

mod file;
mod room;
#[cfg(test)]
mod tests;

pub use file::DiskFile;

use nfs_client::Fh;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

const MIN_FREE: u64 = 1 << 30;

pub struct DiskCache {
    dir: PathBuf,
    limit: u64,
    /// Versions some reader has open, by name.
    open: Mutex<HashMap<String, Weak<file::State>>>,
}

impl DiskCache {
    /// `limit` in bytes; 0 turns the cache off.
    pub fn new(dir: impl Into<PathBuf>, limit: u64) -> io::Result<Arc<Self>> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Arc::new(Self { dir, limit, open: Mutex::default() }))
    }

    fn key(fh: &Fh) -> String {
        let mut hasher = DefaultHasher::new();
        fh.0.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub(crate) fn file(&self, fh: &Fh, version: u64, size: u64) -> Option<DiskFile> {
        if self.limit == 0 {
            return None;
        }
        let key = Self::key(fh);
        let name = format!("{key}-{version:x}");
        let mut open = self.open.lock().expect("not poisoned");
        open.retain(|_, file| file.strong_count() > 0);
        if let Some(state) = open.get(&name).and_then(Weak::upgrade).filter(|s| s.current()) {
            return Some(DiskFile(state));
        }
        self.entries()
            .filter(|(n, ..)| n.starts_with(&key) && *n != name)
            .for_each(|(n, ..)| self.remove(&n));
        self.evict(size);
        let file = DiskFile::open(self.dir.join(&name), size).ok()?;
        open.insert(name, Arc::downgrade(&file.0));
        Some(file)
    }

    /// Every cached version of the file: it was removed or renamed.
    pub(crate) fn forget(&self, fh: &Fh) {
        let key = Self::key(fh);
        self.entries().filter(|(n, ..)| n.starts_with(&key)).for_each(|(n, ..)| self.remove(&n));
    }

    /// How many versions of the file are cached.
    pub fn versions(&self, fh: &Fh) -> usize {
        self.entries().filter(|(n, ..)| n.starts_with(&Self::key(fh))).count()
    }

    /// Bytes on disk.
    pub fn used(&self) -> u64 {
        self.entries().map(|(_, used, _)| used).sum()
    }

    pub fn clear(&self) {
        self.entries().for_each(|(name, ..)| self.remove(&name));
    }
}

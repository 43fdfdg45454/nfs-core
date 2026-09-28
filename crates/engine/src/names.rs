//! Removing and renaming through the engine: the file's pieces are not kept, in memory or on
//! disk, for a name that no longer leads to it.

use crate::Engine;
use nfs_client::{Fh, Result};
use std::sync::Weak;

impl Engine {
    pub(crate) fn forget(&self, fh: &Fh) {
        if let Some(shared) =
            self.open.lock().expect("not poisoned").remove(fh).and_then(|s| Weak::upgrade(&s))
        {
            shared.forget();
        }
        if let Some(cache) = &self.config.cache {
            cache.forget(fh);
        }
    }

    pub async fn remove(&self, dir: &Fh, name: &str) -> Result<()> {
        if let Ok((fh, _)) = self.client.lookup(Some(dir), name).await {
            self.forget(&fh);
        }
        self.client.remove(dir, name).await
    }

    pub async fn rename(&self, from_dir: &Fh, from: &str, to_dir: &Fh, to: &str) -> Result<()> {
        if let Ok((fh, _)) = self.client.lookup(Some(from_dir), from).await {
            self.forget(&fh);
        }
        self.client.rename(from_dir, from, to_dir, to).await
    }

    pub fn cache(&self) -> Option<&std::sync::Arc<crate::DiskCache>> {
        self.config.cache.as_ref()
    }

    /// Bytes of pieces in memory now, and the most there were (all files together).
    pub fn memory(&self) -> (u64, u64) {
        self.memory.used()
    }

    /// Files open for reading now (all the readers of one count once).
    pub fn open_files(&self) -> usize {
        self.open.lock().expect("not poisoned").values().filter(|s| s.strong_count() > 0).count()
    }

    /// Whether nothing is open for reading and no read-ahead is queued: after the last reader
    /// closes, the tests check that nothing leaked.
    pub fn idle(&self) -> bool {
        let open = self.open.lock().expect("not poisoned").values().any(|s| s.strong_count() > 0);
        !open && self.queue.is_empty()
    }
}

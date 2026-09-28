//! The cache's files on disk: listing, removing, and making room (least recently opened first).

use super::{DiskCache, MIN_FREE};
use std::os::unix::fs::MetadataExt;
use std::time::SystemTime;

impl DiskCache {
    /// Each cached file's name, bytes on disk and last opening.
    pub(super) fn entries(&self) -> impl Iterator<Item = (String, u64, SystemTime)> {
        let entries = std::fs::read_dir(&self.dir).into_iter().flatten().flatten();
        entries.filter_map(|e| {
            let name = e.file_name().into_string().ok()?.strip_suffix(".data")?.to_owned();
            let meta = e.metadata().ok()?;
            Some((name, meta.blocks() * 512, meta.modified().ok()?))
        })
    }

    pub(super) fn remove(&self, name: &str) {
        for suffix in [".data", ".index"] {
            _ = std::fs::remove_file(self.dir.join(format!("{name}{suffix}")));
        }
    }

    fn free(&self) -> u64 {
        rustix::fs::statvfs(&self.dir).map_or(u64::MAX, |s| s.f_bavail * s.f_frsize)
    }

    /// Makes room for `size` more bytes.
    pub(super) fn evict(&self, size: u64) {
        let mut entries: Vec<_> = self.entries().collect();
        entries.sort_by_key(|(_, _, opened)| *opened);
        let mut used: u64 = entries.iter().map(|(_, used, _)| used).sum();
        for (name, bytes, _) in entries {
            if used + size <= self.limit && self.free() >= MIN_FREE + size {
                break;
            }
            self.remove(&name);
            used -= bytes;
        }
    }
}

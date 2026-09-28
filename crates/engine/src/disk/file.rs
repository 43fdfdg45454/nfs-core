//! One cached version of a file: a sparse file as large as the remote one, and an index of the
//! pieces written to it, saved once the last reader is gone and every write has landed.

use crate::PIECE;
use bytes::Bytes;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Every reader of a version shares one: two would each save their own index over the other's.
#[derive(Clone)]
pub struct DiskFile(pub(super) Arc<State>);

pub(super) struct State {
    data: File,
    index: PathBuf,
    /// The data file's inode: the index is saved only for this file, not for a new one made at the
    /// same path after the cache removed this one.
    inode: u64,
    /// A bit per piece.
    present: Mutex<Vec<u8>>,
    /// Pieces handed to the disk and not yet written: served from here meanwhile.
    writing: Mutex<HashMap<u64, Bytes>>,
}

impl DiskFile {
    pub fn open(path: PathBuf, size: u64) -> std::io::Result<Self> {
        let data = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("data"))?;
        let index = path.with_extension("index");
        let bytes = size.div_ceil(PIECE).div_ceil(8) as usize;
        // Without a good index (not saved yet, or lost) nothing is taken as present, and the data
        // stays: an index saved late may still mark it. Only a data file of another size (never
        // this version's) is emptied.
        let mut present = std::fs::read(&index).unwrap_or_default();
        if data.metadata()?.len() != size {
            data.set_len(0)?;
            data.set_len(size)?;
            present.clear();
        }
        if present.len() != bytes {
            present = vec![0; bytes];
        }
        data.set_modified(std::time::SystemTime::now())?;
        let (present, writing) = (Mutex::new(present), Mutex::default());
        let inode = data.metadata()?.ino();
        Ok(Self(Arc::new(State { data, index, inode, present, writing })))
    }

    pub fn has(&self, i: u64) -> bool {
        if self.0.writing.lock().expect("not poisoned").contains_key(&i) {
            return true;
        }
        let present = self.0.present.lock().expect("not poisoned");
        present.get((i / 8) as usize).is_some_and(|b| b & (1 << (i % 8)) != 0)
    }

    pub async fn read(&self, i: u64, len: u64) -> Option<Bytes> {
        if let Some(data) = self.0.writing.lock().expect("not poisoned").get(&i) {
            return Some(data.clone());
        }
        if !self.has(i) {
            return None;
        }
        let state = self.0.clone();
        let read = tokio::task::spawn_blocking(move || {
            let mut buf = vec![0; len as usize];
            state.data.read_exact_at(&mut buf, i * PIECE).map(|()| Bytes::from(buf))
        });
        read.await.ok()?.ok()
    }

    /// In the background: the reader already has the data.
    pub fn write(&self, i: u64, piece: Bytes) {
        let state = self.0.clone();
        state.writing.lock().expect("not poisoned").insert(i, piece.clone());
        tokio::task::spawn_blocking(move || {
            if state.data.write_all_at(&piece, i * PIECE).is_ok() {
                let mut present = state.present.lock().expect("not poisoned");
                if let Some(byte) = present.get_mut((i / 8) as usize) {
                    *byte |= 1 << (i % 8);
                }
            }
            state.writing.lock().expect("not poisoned").remove(&i);
        });
    }
}

impl State {
    /// Whether this is still the cache's file at its path (not removed since).
    pub(super) fn current(&self) -> bool {
        std::fs::metadata(self.index.with_extension("data")).is_ok_and(|m| m.ino() == self.inode)
    }
}

/// Saved unless the cache dropped this data file meanwhile (a newer version, removed, evicted), off
/// the closing reader's thread: with much cache being written, the kernel may hold any writer
/// back for a while (stage 3: a close took 629 ms).
impl Drop for State {
    fn drop(&mut self) {
        let index = std::mem::take(&mut self.index);
        let present = std::mem::take(&mut *self.present.lock().expect("not poisoned"));
        let inode = self.inode;
        let save = move || {
            if std::fs::metadata(index.with_extension("data")).is_ok_and(|m| m.ino() == inode) {
                _ = std::fs::write(&index, present);
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(save)),
            Err(_) => save(),
        }
    }
}

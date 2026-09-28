//! Getting pieces: from memory, from a fetch already under way, from disk, or from the server.

use crate::shared::Shared;
use crate::{BLOCK, PIECE, Source};
use bytes::Bytes;
use nfs_client::{Error, Result};
use std::sync::Arc;
use tokio::sync::watch;

impl Shared {
    fn len(&self, i: u64) -> u64 {
        PIECE.min(self.size.saturating_sub(i * PIECE))
    }

    /// Piece `i` for a reader waiting for it, and where it came from: anything missing comes on
    /// the priority lane.
    pub async fn piece(self: &Arc<Self>, i: u64) -> Result<(Bytes, Source)> {
        if let Some(data) = self.pieces.lock().expect("not poisoned").get(i) {
            return Ok((data, Source::Memory));
        }
        let _waiting = Waiting::new(&self.queue.waiting);
        for fetched in [false, true] {
            if i * PIECE >= self.end() {
                return Ok((Bytes::new(), Source::Network));
            }
            let pending = {
                let pieces = self.pieces.lock().expect("not poisoned");
                if let Some(data) = pieces.get(i) {
                    // After fetching it here, it is in memory because the reader waited for it.
                    return Ok((data, if fetched { Source::Network } else { Source::Memory }));
                }
                pieces.pending(i)
            };
            if let Some(mut done) = pending {
                _ = done.wait_for(|done| *done).await;
                if let Some(data) = self.pieces.lock().expect("not poisoned").get(i) {
                    return Ok((data, Source::Network));
                }
            }
            if let Some(data) = self.disk_piece(i).await {
                return Ok((data, Source::Disk));
            }
            if !fetched {
                self.fetch(i, 1, false).await?;
            }
        }
        Err(Error::Other("the file on the server is shorter than it was".into()))
    }

    async fn disk_piece(&self, i: u64) -> Option<Bytes> {
        let data = self.disk.as_ref()?.read(i, self.len(i)).await?;
        self.pieces.lock().expect("not poisoned").put(i, data.clone(), true);
        Some(data)
    }

    /// Fetches, in one READ, the pieces of `first..first + count` neither here nor under way.
    pub async fn fetch(&self, first: u64, count: u64, bulk: bool) -> Result<()> {
        let (done, _) = watch::channel(false);
        let end = (first + count).min(self.size.div_ceil(PIECE));
        let claimed = self.pieces.lock().expect("not poisoned").claim(first..end, &done);
        let (Some(&lo), Some(&hi)) = (claimed.first(), claimed.last()) else { return Ok(()) };
        let offset = lo * PIECE;
        let len = ((hi + 1) * PIECE).min(self.size) - offset;
        let result = self.file.read(offset, len as u32, bulk).await;
        let (data, eof) =
            result.as_ref().map_or((Bytes::new(), false), |(d, eof)| (d.clone(), *eof));
        if eof && offset + (data.len() as u64) < self.size {
            self.shrink(offset + data.len() as u64);
        }
        for i in claimed {
            let start = ((i - lo) * PIECE) as usize;
            let end = (start + self.len(i) as usize).min(data.len());
            let piece = (start < end).then(|| data.slice(start..end));
            let mut pieces = self.pieces.lock().expect("not poisoned");
            match piece {
                // The whole piece, or the last one of a truncated file (kept only in memory).
                Some(piece) if piece.len() as u64 == self.len(i) || eof => {
                    let whole = piece.len() as u64 == self.len(i);
                    if let Some(disk) = self.disk.as_ref().filter(|_| whole && !self.forgotten()) {
                        disk.write(i, piece.clone());
                    }
                    pieces.put(i, piece, self.in_memory(i));
                }
                _ => pieces.fail(i),
            }
        }
        done.send_replace(true);
        result.map(drop)
    }

    /// A block from the read-ahead queue, if some reader still wants it.
    pub async fn fetch_ahead(self: Arc<Self>, block: u64) {
        self.queued.lock().expect("not poisoned").remove(&block);
        if self.wanted(block) {
            _ = self.fetch(block * BLOCK, BLOCK, true).await;
        }
        let keep = |i| self.in_memory(i);
        self.pieces.lock().expect("not poisoned").trim(keep);
    }
}

/// Counts a reader as waiting for as long as it lives.
struct Waiting<'a>(&'a std::sync::atomic::AtomicUsize);

impl<'a> Waiting<'a> {
    fn new(count: &'a std::sync::atomic::AtomicUsize) -> Self {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self(count)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

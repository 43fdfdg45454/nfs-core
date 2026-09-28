//! A reader: one open file descriptor, with its own position, over the file's shared pieces.

use crate::shared::Shared;
use crate::{PIECE, Source, ahead};
use bytes::{Bytes, BytesMut};
use nfs_client::Result;
use std::sync::{Arc, Mutex};

pub struct Reader {
    shared: Arc<Shared>,
    id: u64,
    /// Where the last read ended, bytes read since the last jump, and how far read-ahead reaches.
    state: Mutex<(u64, u64, u64)>,
    /// Where the last read's data came from (the slowest of its pieces).
    source: Mutex<Source>,
}

impl Reader {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        let id = shared.add_reader();
        Self { shared, id, state: Mutex::new((0, 0, 0)), source: Mutex::new(Source::Memory) }
    }

    pub fn size(&self) -> u64 {
        self.shared.size
    }

    /// Where the last read's data came from: the tests check the cache with it.
    pub fn last_source(&self) -> Source {
        *self.source.lock().expect("not poisoned")
    }

    async fn piece(&self, i: u64, worst: &mut Source) -> Result<Bytes> {
        let (data, source) = self.shared.piece(i).await?;
        *worst = (*worst).max(source);
        Ok(data)
    }

    /// Up to `len` bytes at `offset`: fewer at the end of the file, which is asked again (someone
    /// else may have made it longer since it was opened).
    pub async fn read_at(&self, offset: u64, len: usize) -> Result<Bytes> {
        let (end, known) = (offset + len as u64, self.shared.end());
        let data = self.pieces(offset, end.min(known)).await?;
        let from = offset + data.len() as u64;
        if end <= known || from < known || known < self.shared.size {
            return Ok(data);
        }
        let (more, _) = self.shared.file.read(from, (end - from) as u32, false).await?;
        *self.source.lock().expect("not poisoned") = Source::Network;
        Ok(if data.is_empty() { more } else { [data, more].concat().into() })
    }

    async fn pieces(&self, offset: u64, end: u64) -> Result<Bytes> {
        if offset >= end {
            return Ok(Bytes::new());
        }
        self.moved(offset, end);
        let (first, last) = (offset / PIECE, (end - 1) / PIECE);
        let slice = |i: u64, data: &Bytes| {
            let from = offset.saturating_sub(i * PIECE) as usize;
            let to = (end - i * PIECE).min(data.len() as u64) as usize;
            data.slice(from.min(to)..to)
        };
        let mut worst = Source::Memory;
        let data = if first == last {
            slice(first, &self.piece(first, &mut worst).await?)
        } else {
            let mut out = BytesMut::with_capacity((end - offset) as usize);
            for i in first..=last {
                out.extend_from_slice(&slice(i, &self.piece(i, &mut worst).await?));
            }
            out.freeze()
        };
        *self.source.lock().expect("not poisoned") = worst;
        Ok(data)
    }

    /// Notes the new position (a read within a piece of where the last ended continues the run:
    /// the file proxy reads page-aligned) and queues read-ahead before waiting for anything.
    fn moved(&self, offset: u64, end: u64) {
        let mut state = self.state.lock().expect("not poisoned");
        let (last, run, scheduled) = &mut *state;
        let continues = offset + PIECE >= *last && offset <= *last + PIECE;
        *run = if continues { *run + (end - offset) } else { end - offset };
        if !continues {
            *scheduled = 0;
        }
        *last = end;
        self.shared.set_reader(self.id, end, *run);
        ahead::schedule(&self.shared, end, *run, scheduled);
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.shared.remove_reader(self.id);
    }
}

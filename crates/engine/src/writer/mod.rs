//! A file written through the engine: data gathered into WRITEs of the session's size, sent in
//! parallel off the priority lane, and kept until a COMMIT confirms it. A write verifier that
//! changed (the server restarted and lost unstable data) sends that data again.

use crate::Engine;
use bytes::{Bytes, BytesMut};
use nfs_client::attr::Attrs;
use nfs_client::{Create, Fh, File, Result, WRITE_ACCESS};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

mod flush;

/// Unconfirmed data kept before a COMMIT is asked for.
pub(super) const UNCONFIRMED: usize = 64 << 20;

pub(super) type Sent = Vec<(u64, Bytes, [u8; 8])>;

pub struct Writer {
    pub(super) file: Arc<File>,
    pub(super) chunk: usize,
    /// Data not sent yet: its offset, and bytes that follow each other.
    pub(super) buffer: Mutex<(u64, BytesMut)>,
    pub(super) sent: Arc<Mutex<Sent>>,
    pub(super) tasks: Mutex<JoinSet<Result<()>>>,
    pub(super) permits: Arc<Semaphore>,
    pub(super) limit: usize,
    pub(super) queue: Arc<crate::queue::Queue>,
}

impl Engine {
    pub async fn create(
        self: &Arc<Self>,
        dir: &Fh,
        name: &str,
        create: Create,
    ) -> Result<(Writer, Attrs)> {
        let (file, attrs) = self.client.create(dir, name, create, WRITE_ACCESS).await?;
        self.forget(&file.fh);
        Ok((self.writer(file), attrs))
    }

    /// Writing an existing file (reading it too, with `read`): what is kept of it for reading is
    /// dropped (a delegation the client holds is not recalled for its own writes).
    pub async fn write(self: &Arc<Self>, fh: &Fh, read: bool) -> Result<Writer> {
        self.forget(fh);
        let access = if read { WRITE_ACCESS | nfs_client::READ_ACCESS } else { WRITE_ACCESS };
        Ok(self.writer(self.client.open(fh, access).await?))
    }

    fn writer(&self, file: File) -> Writer {
        let (chunk, file) = (self.client.max_io() as usize, Arc::new(file));
        let (buffer, sent, tasks) = Default::default();
        Writer {
            file,
            chunk,
            buffer,
            sent,
            tasks,
            permits: Arc::new(Semaphore::new(self.config.in_flight)),
            limit: self.config.in_flight,
            queue: self.queue.clone(),
        }
    }
}

impl Writer {
    pub async fn write_at(&self, offset: u64, data: &[u8]) -> Result<()> {
        let full = {
            let mut buffer = self.buffer.lock().expect("not poisoned");
            let (start, bytes) = &mut *buffer;
            let mut out = Vec::new();
            if *start + bytes.len() as u64 != offset && !bytes.is_empty() {
                out.push((*start, bytes.split().freeze()));
            }
            if bytes.is_empty() {
                *start = offset;
            }
            bytes.extend_from_slice(data);
            while bytes.len() >= self.chunk {
                out.push((*start, bytes.split_to(self.chunk).freeze()));
                *start += self.chunk as u64;
            }
            out
        };
        for (offset, data) in full {
            self.send(offset, data).await;
        }
        let unconfirmed: usize =
            self.sent.lock().expect("not poisoned").iter().map(|(_, d, _)| d.len()).sum();
        if unconfirmed >= UNCONFIRMED { self.flush().await } else { Ok(()) }
    }

    pub async fn close(self) -> Result<()> {
        self.flush().await?;
        self.file.close().await
    }
}

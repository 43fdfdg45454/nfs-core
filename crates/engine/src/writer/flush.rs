//! Sending gathered data, and confirming it with COMMIT (sending again what a restarted server
//! lost).

use super::{Sent, Writer};
use bytes::Bytes;
use nfs_client::{Error, Result};

impl Writer {
    pub(super) async fn send(&self, offset: u64, data: Bytes) {
        let permit = self.permits.clone().acquire_owned().await.expect("never closed");
        // While a reader waits for the network, at most 2 writes are in flight: a write queued on
        // every connection would make the reader's request wait behind it (stage 3, over TCP:
        // jumps while uploading took 3.6 s on average).
        let busy = || self.limit - self.permits.available_permits() > 2;
        while busy() && self.queue.waiting.load(std::sync::atomic::Ordering::Relaxed) > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let (file, sent) = (self.file.clone(), self.sent.clone());
        self.tasks.lock().expect("not poisoned").spawn(async move {
            let (count, verifier) = file.write(offset, &data).await?;
            drop(permit);
            if (count as usize) < data.len() {
                return Err(Error::Other(format!(
                    "the server wrote {count} of {} bytes",
                    data.len()
                )));
            }
            sent.lock().expect("not poisoned").push((offset, data, verifier));
            Ok(())
        });
    }

    /// Sends what is buffered and waits until the server has it (not yet on stable storage).
    async fn send_all(&self) -> Result<()> {
        let rest = std::mem::take(&mut *self.buffer.lock().expect("not poisoned"));
        if !rest.1.is_empty() {
            self.send(rest.0, rest.1.freeze()).await;
        }
        let mut tasks = std::mem::take(&mut *self.tasks.lock().expect("not poisoned"));
        while let Some(done) = tasks.join_next().await {
            done.map_err(|e| Error::Other(e.to_string()))??;
        }
        Ok(())
    }

    /// Up to `len` bytes at `offset`, with what this writer wrote (the file must be open for
    /// reading too: `Engine::write` with `read`).
    pub async fn read_at(&self, offset: u64, len: u32) -> Result<Bytes> {
        self.send_all().await?;
        let (mut out, mut at) = (bytes::BytesMut::new(), offset);
        while out.len() < len as usize {
            let (data, eof) = self.file.read(at, len - out.len() as u32, false).await?;
            out.extend_from_slice(&data);
            at += data.len() as u64;
            if eof || data.is_empty() {
                break;
            }
        }
        Ok(out.freeze())
    }

    /// Sends what is buffered and waits until the server has all of it on stable storage.
    pub async fn flush(&self) -> Result<()> {
        self.send_all().await?;
        for _ in 0..3 {
            let mut tasks = std::mem::take(&mut *self.tasks.lock().expect("not poisoned"));
            while let Some(done) = tasks.join_next().await {
                done.map_err(|e| Error::Other(e.to_string()))??;
            }
            let verifier = self.file.commit().await?;
            let stale: Sent = std::mem::take(&mut *self.sent.lock().expect("not poisoned"));
            let stale: Vec<_> = stale.into_iter().filter(|(_, _, v)| *v != verifier).collect();
            if stale.is_empty() {
                return Ok(());
            }
            for (offset, data, _) in stale {
                self.send(offset, data).await;
            }
        }
        Err(Error::Other("the server keeps losing written data".into()))
    }
}

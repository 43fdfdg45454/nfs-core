//! COMMITs while writing goes on: what the server already has is confirmed in the background, so
//! the upload never waits for the server's disk (it did every 64 MiB, the pipeline drained).

use super::{Sent, Writer};
use nfs_client::{Error, Result};
use tokio::task::JoinHandle;

pub(super) type Committing = std::sync::Mutex<Option<JoinHandle<Result<Sent>>>>;

impl Writer {
    /// Confirms what was sent and acknowledged so far with a COMMIT of its own. One at a time:
    /// the next waits for this one, so at most twice UNCONFIRMED is kept unconfirmed.
    pub(super) async fn commit_behind(&self) -> Result<()> {
        self.settle().await?;
        let batch: Sent = std::mem::take(&mut *self.sent.lock().expect("not poisoned"));
        let file = self.file.clone();
        let task = tokio::spawn(async move {
            let verifier = file.commit().await?;
            // Written before the server restarted: lost, to send again.
            Ok(batch.into_iter().filter(|(_, _, v)| *v != verifier).collect())
        });
        *self.committing.lock().expect("not poisoned") = Some(task);
        Ok(())
    }

    /// Waits for a COMMIT under way, and sends again what it found lost.
    pub(super) async fn settle(&self) -> Result<()> {
        let running = self.committing.lock().expect("not poisoned").take();
        let Some(task) = running else { return Ok(()) };
        let stale = task.await.map_err(|e| Error::Other(e.to_string()))??;
        for (offset, data, _) in stale {
            self.send(offset, data).await;
        }
        Ok(())
    }
}

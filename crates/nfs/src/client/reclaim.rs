//! An open file after the server restarted: its open and its locks, asked back in the grace
//! period (CLAIM_PREVIOUS, LOCK with reclaim).

use super::OpenFile;
use crate::compound::Ops;
use crate::error::Result;
use crate::ops::lock::{Locker, Range};
use crate::ops::open::Claim;
use crate::ops::{LOCK, PUTFH};
use crate::session::Reclaim;
use crate::types::Stateid;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

impl Reclaim for OpenFile {
    fn reclaim(self: Arc<Self>) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { self.reclaim_state().await })
    }
}

impl OpenFile {
    async fn reclaim_state(&self) {
        if self.reopen_with(Claim::Previous).await.is_err() {
            return;
        }
        let ranges = {
            let mut locks = self.locks.lock().expect("not poisoned");
            locks.stateid = None;
            locks.held.ranges()
        };
        for (start, end, kind) in ranges {
            let length = if end == u64::MAX { u64::MAX } else { end - start };
            match self.reclaim_lock(kind, Range { offset: start, length }).await {
                Ok(stateid) => self.locks.lock().expect("not poisoned").stateid = Some(stateid),
                Err(_) => {
                    let mut locks = self.locks.lock().expect("not poisoned");
                    locks.lost = true;
                    locks.held.set(start, end, None);
                }
            }
        }
    }

    async fn reclaim_lock(&self, kind: crate::LockKind, range: Range) -> Result<Stateid> {
        let (clientid, owner) = (self.client.session.clientid().await, self.lock_owner());
        let locker = match self.lock_stateid() {
            Some(stateid) => Locker::Existing(stateid),
            None => Locker::New { open: self.stateid(), clientid, owner: &owner },
        };
        let mut ops = Ops::default();
        ops.putfh(&self.fh).lock(kind, false, true, &range, locker);
        let mut r = self.client.call(&ops).await?;
        r.next(PUTFH)?;
        Ok(Stateid::decode(r.next(LOCK)?)?)
    }
}

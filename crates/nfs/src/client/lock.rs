//! Byte-range locks on an open file. A lock someone else holds is waited for when asked: the
//! server says when it may be free (CB_NOTIFY_LOCK) and, in case it does not, it is tried again
//! with a growing pause.

use super::OpenFile;
use super::held::end;
use crate::compound::Ops;
use crate::error::{Error, Result};
use crate::ops::lock::{LockKind, Locker, Range};
use crate::ops::{LOCK, LOCKT, LOCKU, PUTFH};
use crate::status::Status;
use crate::types::Stateid;
use std::time::Duration;

impl OpenFile {
    pub(super) fn lock_owner(&self) -> Vec<u8> {
        [b"lock-", &self.owner[..]].concat()
    }

    /// Locks `length` bytes from `offset` (u64::MAX: to the end of the file) as `kind` (a read
    /// lock needs the file open for reading, a write lock for writing: NFS4ERR_OPENMODE). Without
    /// `wait`, a conflicting lock fails with NFS4ERR_DENIED; with it, it is waited for.
    pub async fn lock(&self, kind: LockKind, offset: u64, length: u64, wait: bool) -> Result<()> {
        // A server that notifies wakes the waiter at once: its polls are only a fallback.
        let notifies = self.opened.lock().expect("not poisoned").notifies_locks;
        let longest = Duration::from_secs(if notifies { 15 } else { 4 });
        let mut pause = Duration::from_millis(250);
        loop {
            let wake = self.client.session.callbacks.lock_wait(&self.fh);
            let notified = wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            match self.try_lock(kind, Range { offset, length }, wait).await {
                Err(Error::Nfs(Status::DENIED)) if wait => {
                    tokio::select! {
                        _ = notified => {}
                        _ = tokio::time::sleep(pause) => pause = (pause * 2).min(longest),
                    }
                }
                result => return result,
            }
        }
    }

    async fn try_lock(&self, kind: LockKind, range: Range, wait: bool) -> Result<()> {
        let _one = self.lock_calls.lock().await;
        for attempt in 0..3 {
            let (clientid, owner) = (self.client.session.clientid().await, self.lock_owner());
            let (lock, open) = (self.lock_stateid(), self.stateid());
            let locker = match lock {
                Some(stateid) => Locker::Existing(stateid),
                None => Locker::New { open, clientid, owner: &owner },
            };
            let mut ops = Ops::default();
            ops.putfh(&self.fh).lock(kind, wait, false, &range, locker);
            let mut r = self.client.call(&ops).await?;
            let status = r.status();
            if status.lost_state() && (self.lock_stateid(), self.stateid()) != (lock, open) {
                continue;
            }
            if status.lost_state() && attempt == 0 {
                self.locks_gone();
                self.reopen().await?;
                continue;
            }
            r.next(PUTFH)?;
            let stateid = Stateid::decode(r.next(LOCK)?)?;
            let mut locks = self.locks.lock().expect("not poisoned");
            locks.stateid = Some(stateid);
            locks.held.set(range.offset, end(range.offset, range.length), Some(kind));
            return Ok(());
        }
        Err(Error::Other("the lock state was lost again".into()))
    }

    pub(super) fn lock_stateid(&self) -> Option<Stateid> {
        self.locks.lock().expect("not poisoned").stateid
    }

    pub async fn unlock(&self, offset: u64, length: u64) -> Result<()> {
        let _one = self.lock_calls.lock().await;
        let Some(stateid) = self.lock_stateid() else { return Ok(()) };
        let mut ops = Ops::default();
        ops.putfh(&self.fh).locku(LockKind::Write, &stateid, &Range { offset, length });
        let mut r = self.client.call(&ops).await?;
        r.next(PUTFH)?;
        let stateid = Stateid::decode(r.next(LOCKU)?)?;
        let mut locks = self.locks.lock().expect("not poisoned");
        locks.stateid = Some(stateid);
        locks.held.set(offset, end(offset, length), None);
        Ok(())
    }

    /// Whether `kind` could be locked there now: no other owner holds a conflicting lock.
    pub async fn can_lock(&self, kind: LockKind, offset: u64, length: u64) -> Result<bool> {
        let clientid = self.client.session.clientid().await;
        let mut ops = Ops::default();
        ops.putfh(&self.fh).lockt(kind, &Range { offset, length }, clientid, &self.lock_owner());
        let mut r = self.client.call(&ops).await?;
        r.next(PUTFH)?;
        match r.next(LOCKT) {
            Ok(_) => Ok(true),
            Err(Error::Nfs(Status::DENIED)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Whether the server tells this client when a lock it waits for may be free.
    pub fn notifies_locks(&self) -> bool {
        self.opened.lock().expect("not poisoned").notifies_locks
    }

    /// Whether the server lost this file's locks (its lease expired) since they were taken.
    pub fn locks_lost(&self) -> bool {
        self.locks.lock().expect("not poisoned").lost
    }

    pub(super) fn locks_gone(&self) {
        let mut locks = self.locks.lock().expect("not poisoned");
        locks.stateid = None;
        locks.lost |= !locks.held.is_empty();
        locks.held = Default::default();
    }

    /// Before CLOSE, which fails while locks are held: all of them go, and the lock state too.
    pub(super) async fn release_locks(&self) {
        let _one = self.lock_calls.lock().await;
        let Some(stateid) = self.locks.lock().expect("not poisoned").stateid.take() else { return };
        let mut ops = Ops::default();
        let all = Range { offset: 0, length: u64::MAX };
        ops.putfh(&self.fh).locku(LockKind::Write, &stateid, &all);
        if let Ok(mut r) = self.client.call(&ops).await
            && r.next(PUTFH).is_ok()
            && let Ok(d) = r.next(LOCKU)
            && let Ok(stateid) = Stateid::decode(d)
        {
            let mut ops = Ops::default();
            ops.free_stateid(&stateid);
            _ = self.client.call(&ops).await;
        }
    }
}

//! Open files: reads and writes with the open's stateid, reopened by handle when the server lost
//! the open state (a new session after an outage, a lease that expired).

use super::Client;
use crate::compound::{Ops, Results};
use crate::error::{Error, Result};
use crate::ops::open::{Claim, Create, Delegation, Opened};
use crate::ops::{self, CLOSE, COMMIT, OPEN, PUTFH, READ, WRITE};
use crate::types::{Fh, Stateid};
use bytes::Bytes;
use std::sync::{Arc, Mutex};

/// An open file. Its state is shared with the session's list of open files, which reclaims it
/// after the server restarts.
pub struct File(Arc<OpenFile>);

impl std::ops::Deref for File {
    type Target = OpenFile;
    fn deref(&self) -> &OpenFile {
        &self.0
    }
}

pub struct OpenFile {
    pub(super) client: Arc<Client>,
    pub fh: Fh,
    pub(super) access: u32,
    pub(super) owner: Vec<u8>,
    /// What the last OPEN gave: the open stateid, a delegation, whether locks are notified.
    pub(super) opened: Mutex<Opened>,
    /// The lock stateid (after the first lock) and the ranges held.
    pub(super) locks: Mutex<Locks>,
    /// One lock call at a time.
    pub(super) lock_calls: tokio::sync::Mutex<()>,
}

#[derive(Default)]
pub(super) struct Locks {
    pub stateid: Option<Stateid>,
    pub held: super::held::Held,
    /// The server lost them (its lease expired): the application may need to know.
    pub lost: bool,
}

impl File {
    pub(super) fn new(
        client: &Arc<Client>,
        fh: Fh,
        access: u32,
        owner: Vec<u8>,
        o: Opened,
    ) -> Self {
        client.opened(&fh, &o);
        let (opened, locks, lock_calls) = (o.into(), Default::default(), Default::default());
        let file = Arc::new(OpenFile {
            client: client.clone(),
            fh,
            access,
            owner,
            opened,
            locks,
            lock_calls,
        });
        client.session.opened(&file);
        File(file)
    }
}

impl OpenFile {
    pub(super) async fn reopen(&self) -> Result<()> {
        self.reopen_with(Claim::Fh).await
    }

    pub(super) async fn reopen_with(&self, claim: Claim<'_>) -> Result<()> {
        let clientid = self.client.session.clientid().await;
        let mut ops = Ops::default();
        ops.putfh(&self.fh).open(clientid, &self.owner, self.access, &Create::No, claim);
        let mut r = self.client.call(&ops).await?;
        r.next(PUTFH)?;
        let opened = ops::open::open(r.next(OPEN)?)?;
        self.client.opened(&self.fh, &opened);
        *self.opened.lock().expect("not poisoned") = opened;
        Ok(())
    }

    pub(super) fn stateid(&self) -> Stateid {
        self.opened.lock().expect("not poisoned").stateid
    }

    /// The read delegation this file was opened with, if any.
    pub fn delegation(&self) -> Option<Stateid> {
        match self.opened.lock().expect("not poisoned").delegation {
            Some(Delegation::Read(stateid)) => Some(stateid),
            _ => None,
        }
    }

    /// Runs `add` (after PUTFH) with the current stateid; if the server lost the open state,
    /// reopens once and runs it again.
    async fn with_state(&self, bulk: bool, add: impl Fn(&mut Ops, &Stateid)) -> Result<Results> {
        for attempt in 0..3 {
            let (mut ops, sent) = (Ops::default(), self.stateid());
            ops.bulk = bulk;
            add(ops.putfh(&self.fh), &sent);
            let mut r = self.client.call(&ops).await?;
            match r.status() {
                // Reclaimed after a restart meanwhile: the new stateid is good.
                status if status.lost_state() && self.stateid() != sent => {}
                status if status.lost_state() && attempt == 0 => {
                    self.reopen().await?;
                    self.locks_gone();
                }
                _ => {
                    r.next(PUTFH)?;
                    return Ok(r);
                }
            }
        }
        Err(Error::Other("the open state was lost again".into()))
    }

    /// Up to `count` bytes (at most the client's max_io) at `offset`, and whether the file ends.
    /// `bulk`: nobody waits for it now (read-ahead), so it stays off the priority lane.
    pub async fn read(&self, offset: u64, count: u32, bulk: bool) -> Result<(Bytes, bool)> {
        let count = count.min(self.client.max_io);
        let mut r = self.with_state(bulk, |ops, s| ops.read(s, offset, count)).await?;
        let (eof, data) = ops::file::read(r.next(READ)?)?;
        Ok((data, eof))
    }

    /// Unstable: the data is safe once `commit` returns the same verifier as the write.
    pub async fn write(&self, offset: u64, data: &[u8]) -> Result<(u32, [u8; 8])> {
        Ok(ops::file::write(
            self.with_state(true, |ops, s| ops.write(s, offset, data)).await?.next(WRITE)?,
        )?)
    }

    pub async fn commit(&self) -> Result<[u8; 8]> {
        Ok(ops::file::commit(
            self.with_state(false, |ops, _| ops.commit(0, 0)).await?.next(COMMIT)?,
        )?)
    }

    /// Closes the open on the server (the handle stays usable for nothing else).
    pub async fn close(&self) -> Result<()> {
        self.release_locks().await;
        let mut r = self.with_state(false, |ops, s| ops.close(s)).await?;
        match r.next(CLOSE) {
            Ok(_) | Err(Error::Nfs(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

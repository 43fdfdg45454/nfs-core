//! Byte-range locks (RFC 8881 sections 18.10-18.12, 18.38).

use super::*;
use crate::types::Stateid;

/// A shared (read) or exclusive (write) lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockKind {
    Read,
    Write,
}

impl LockKind {
    /// READ_LT or WRITE_LT; the W variants tell the server this client will wait, so that it
    /// may notify it (CB_NOTIFY_LOCK).
    fn code(self, wait: bool) -> u32 {
        match self {
            Self::Read => 1 + 2 * u32::from(wait),
            Self::Write => 2 + 2 * u32::from(wait),
        }
    }
}

/// Who asks for the lock: its first lock goes with the open it belongs to.
pub enum Locker<'a> {
    New { open: Stateid, clientid: u64, owner: &'a [u8] },
    Existing(Stateid),
}

pub struct Range {
    pub offset: u64,
    /// u64::MAX: to the end of the file, however far it grows.
    pub length: u64,
}

impl Ops {
    pub fn lock(
        &mut self,
        kind: LockKind,
        wait: bool,
        reclaim: bool,
        range: &Range,
        locker: Locker,
    ) {
        let e =
            self.op(LOCK).u32(kind.code(wait)).bool(reclaim).u64(range.offset).u64(range.length);
        match locker {
            Locker::New { open, clientid, owner } => {
                open.encode(e.bool(true).u32(0));
                e.u32(0).u64(clientid).opaque(owner);
            }
            Locker::Existing(stateid) => {
                stateid.encode(e.bool(false));
                e.u32(0);
            }
        }
        self.modifies = true;
    }

    pub fn lockt(&mut self, kind: LockKind, range: &Range, clientid: u64, owner: &[u8]) {
        let e = self.op(LOCKT).u32(kind.code(false)).u64(range.offset).u64(range.length);
        e.u64(clientid).opaque(owner);
    }

    pub fn locku(&mut self, kind: LockKind, stateid: &Stateid, range: &Range) {
        let e = self.op(LOCKU).u32(kind.code(false)).u32(0);
        stateid.encode(e);
        e.u64(range.offset).u64(range.length);
        self.modifies = true;
    }

    pub fn free_stateid(&mut self, stateid: &Stateid) {
        stateid.encode(self.op(FREE_STATEID));
        self.modifies = true;
    }
}

//! Operation arguments, added to a COMPOUND, and the decoding of their results. File handle
//! operations live here; the rest by subject.

pub mod copy;
pub mod dir;
pub mod file;
pub mod lock;
pub mod open;
pub mod session;

use crate::compound::Ops;
use crate::types::Fh;
use nfs_xdr::{Decoder, Result};

pub const CLOSE: u32 = 4;
pub const COMMIT: u32 = 5;
pub const CREATE: u32 = 6;
pub const DELEGRETURN: u32 = 8;
pub const GETATTR: u32 = 9;
pub const GETFH: u32 = 10;
pub const LINK: u32 = 11;
pub const LOCK: u32 = 12;
pub const LOCKT: u32 = 13;
pub const LOCKU: u32 = 14;
pub const LOOKUP: u32 = 15;
pub const OPEN: u32 = 18;
pub const PUTFH: u32 = 22;
pub const PUTROOTFH: u32 = 24;
pub const READ: u32 = 25;
pub const READDIR: u32 = 26;
pub const READLINK: u32 = 27;
pub const REMOVE: u32 = 28;
pub const RENAME: u32 = 29;
pub const SAVEFH: u32 = 32;
pub const SETATTR: u32 = 34;
pub const WRITE: u32 = 38;
pub const BIND_CONN_TO_SESSION: u32 = 41;
pub const EXCHANGE_ID: u32 = 42;
pub const FREE_STATEID: u32 = 45;
pub const CREATE_SESSION: u32 = 43;
pub const SEQUENCE: u32 = 53;
pub const RECLAIM_COMPLETE: u32 = 58;
pub const COPY: u32 = 60;
pub const CLONE: u32 = 71;

impl Ops {
    pub fn putrootfh(&mut self) -> &mut Self {
        self.op(PUTROOTFH);
        self
    }

    pub fn putfh(&mut self, fh: &Fh) -> &mut Self {
        self.op(PUTFH).opaque(&fh.0);
        self
    }

    pub fn getfh(&mut self) -> &mut Self {
        self.op(GETFH);
        self
    }

    pub fn lookup(&mut self, name: &str) -> &mut Self {
        self.op(LOOKUP).string(name);
        self
    }

    pub fn getattr(&mut self, bits: &[u32]) -> &mut Self {
        self.op(GETATTR).bitmap(&crate::attr::bitmap(bits));
        self
    }

    pub fn savefh(&mut self) -> &mut Self {
        self.op(SAVEFH);
        self
    }
}

pub fn fh(d: &mut Decoder) -> Result<Fh> {
    Ok(Fh(d.opaque()?))
}

/// change_info4 of a directory changed by the operation: not used.
pub fn skip_change_info(d: &mut Decoder) -> Result<()> {
    d.bool()?;
    d.u64()?;
    d.u64().map(drop)
}

//! Opening and closing files (RFC 8881 sections 18.16, 18.2).

use super::*;
use crate::attr::SetAttrs;
use crate::types::Stateid;

pub const READ_ACCESS: u32 = 1;
pub const WRITE_ACCESS: u32 = 2;
/// A read-only open asks for a read delegation (OPEN4_SHARE_ACCESS_WANT_READ_DELEG): while the
/// client holds it, nobody else changes the file, so what was read stays valid without asking.
/// Other opens ask for none (OPEN4_SHARE_ACCESS_WANT_NO_DELEG): a write delegation would make the
/// client answer for the file's attributes and flush on recall.
const WANT_READ_DELEG: u32 = 0x0100;
const WANT_NO_DELEG: u32 = 0x0400;
/// OPEN4_RESULT_MAY_NOTIFY_LOCK: the server tells when a lock someone waits for may be free.
const MAY_NOTIFY_LOCK: u32 = 0x20;
/// How OPEN treats a missing or existing file.
#[derive(Debug, Clone)]
pub enum Create {
    /// The file must exist.
    No,
    /// Created if missing; `attrs` applies only then.
    Unchecked(SetAttrs),
    /// Created, failing with NFS4ERR_EXIST if it exists (O_EXCL). GUARDED4, not EXCLUSIVE4_1:
    /// the latter stores its verifier in the file's times until the client sets them.
    Guarded(SetAttrs),
}

/// What OPEN opens: `name` in the current file handle (a directory), or the current file itself.
pub enum Claim<'a> {
    Name(&'a str),
    Fh,
    /// After the server restarted, in its grace period: the open the client had (no delegation).
    Previous,
}

impl Ops {
    pub fn open(
        &mut self,
        clientid: u64,
        owner: &[u8],
        access: u32,
        create: &Create,
        claim: Claim,
    ) {
        let want = if access == READ_ACCESS { WANT_READ_DELEG } else { WANT_NO_DELEG };
        let e = self.op(OPEN).u32(0).u32(access | want).u32(0).u64(clientid).opaque(owner);
        match create {
            Create::No => {
                e.u32(0);
            }
            Create::Unchecked(attrs) | Create::Guarded(attrs) => {
                e.u32(1).u32(if matches!(create, Create::Guarded(_)) { 1 } else { 0 });
                attrs.encode(e);
            }
        }
        match claim {
            Claim::Name(name) => e.u32(0).string(name),
            Claim::Fh => e.u32(4),
            Claim::Previous => e.u32(1).u32(0),
        };
        self.modifies = true;
    }

    pub fn close(&mut self, stateid: &Stateid) {
        stateid.encode(self.op(CLOSE).u32(0));
        self.modifies = true;
    }
}

/// What OPEN gave.
#[derive(Debug, Clone, Copy, Default)]
pub struct Opened {
    pub stateid: Stateid,
    pub delegation: Option<Delegation>,
    /// The server sends CB_NOTIFY_LOCK when a lock a client waits for may be free.
    pub notifies_locks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delegation {
    Read(Stateid),
    /// Never asked for: returned at once.
    Write(Stateid),
}

pub fn open(d: &mut Decoder) -> Result<Opened> {
    let stateid = Stateid::decode(d)?;
    skip_change_info(d)?;
    let flags = d.u32()?;
    d.bitmap()?;
    let delegation = match d.u32()? {
        1 => Some(Delegation::Read(read_delegation(d)?)),
        2 => Some(Delegation::Write(write_delegation(d)?)),
        3 => {
            if matches!(d.u32()?, 1 | 2) {
                d.bool()?;
            }
            None
        }
        _ => None,
    };
    Ok(Opened { stateid, delegation, notifies_locks: flags & MAY_NOTIFY_LOCK != 0 })
}

/// The stateid, whether it is being recalled already, and the access it grants (nfsace4).
fn read_delegation(d: &mut Decoder) -> Result<Stateid> {
    let stateid = Stateid::decode(d)?;
    d.bool()?;
    skip_ace(d)?;
    Ok(stateid)
}

fn write_delegation(d: &mut Decoder) -> Result<Stateid> {
    let stateid = Stateid::decode(d)?;
    d.bool()?;
    // space_limit4: a size (u64) or a block count and size (two u32), 8 bytes either way.
    d.u32()?;
    d.u64()?;
    skip_ace(d)?;
    Ok(stateid)
}

fn skip_ace(d: &mut Decoder) -> Result<()> {
    d.u32()?;
    d.u32()?;
    d.u32()?;
    d.opaque().map(drop)
}

impl Ops {
    /// Gives back a delegation (RFC 8881 section 18.6), after PUTFH.
    pub fn delegreturn(&mut self, stateid: &Stateid) {
        stateid.encode(self.op(DELEGRETURN));
        self.modifies = true;
    }
}

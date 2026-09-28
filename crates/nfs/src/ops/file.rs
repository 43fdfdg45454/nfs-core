//! Reading, writing, committing and setting attributes (RFC 8881 sections 18.22, 18.32, 18.3,
//! 18.30).

use super::*;
use crate::attr::SetAttrs;
use crate::types::Stateid;
use bytes::Bytes;

/// stable_how4: the server may keep data in memory until COMMIT.
pub const UNSTABLE: u32 = 0;

impl Ops {
    pub fn read(&mut self, stateid: &Stateid, offset: u64, count: u32) {
        let e = self.op(READ);
        stateid.encode(e);
        e.u64(offset).u32(count);
    }

    pub fn write(&mut self, stateid: &Stateid, offset: u64, data: &[u8]) {
        let e = self.op(WRITE);
        stateid.encode(e);
        e.u64(offset).u32(UNSTABLE).opaque(data);
    }

    pub fn commit(&mut self, offset: u64, count: u32) {
        self.op(COMMIT).u64(offset).u32(count);
    }

    pub fn setattr(&mut self, stateid: &Stateid, attrs: &SetAttrs) {
        let e = self.op(SETATTR);
        stateid.encode(e);
        attrs.encode(e);
        self.modifies = true;
    }
}
/// Whether the file ends here, and the data.
pub fn read(d: &mut Decoder) -> Result<(bool, Bytes)> {
    Ok((d.bool()?, d.opaque()?))
}

/// Bytes written, and the write verifier (it changes when the server restarts).
pub fn write(d: &mut Decoder) -> Result<(u32, [u8; 8])> {
    let count = d.u32()?;
    d.u32()?;
    Ok((count, d.opaque_fixed(8)?[..].try_into().unwrap_or_default()))
}

pub fn commit(d: &mut Decoder) -> Result<[u8; 8]> {
    Ok(d.opaque_fixed(8)?[..].try_into().unwrap_or_default())
}

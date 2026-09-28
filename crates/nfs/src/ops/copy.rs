//! Server-side copies (RFC 7862 sections 15.2, 15.13): COPY between the saved file handle
//! (source) and the current one, and CLONE, which shares blocks where the file system can.

use super::*;
use crate::types::Stateid;

impl Ops {
    /// Synchronous, so the reply says how much was copied.
    pub fn copy(
        &mut self,
        from: &Stateid,
        to: &Stateid,
        from_offset: u64,
        to_offset: u64,
        count: u64,
    ) {
        let e = self.op(COPY);
        from.encode(e);
        to.encode(e);
        e.u64(from_offset).u64(to_offset).u64(count).bool(false).bool(true).u32(0);
        self.modifies = true;
    }

    pub fn clone(
        &mut self,
        from: &Stateid,
        to: &Stateid,
        from_offset: u64,
        to_offset: u64,
        count: u64,
    ) {
        let e = self.op(CLONE);
        from.encode(e);
        to.encode(e);
        e.u64(from_offset).u64(to_offset).u64(count);
        self.modifies = true;
    }
}

/// Bytes copied.
pub fn copy(d: &mut Decoder) -> Result<u64> {
    if d.u32()? == 1 {
        Stateid::decode(d)?;
    }
    d.u64()
}

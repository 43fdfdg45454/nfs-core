//! COMPOUND (RFC 8881 section 16.2): operations sent together, run in order until one fails.

use crate::error::{Error, Result};
use crate::status::Status;
use nfs_xdr::{Decoder, Encoder};

pub const PROCEDURE: u32 = 1;

/// The operations of one COMPOUND, encoded as they are added.
#[derive(Default)]
pub struct Ops {
    ops: Encoder,
    count: u32,
    /// Whether the server must keep the reply for a replay (sa_cachethis).
    pub modifies: bool,
    /// Bulk data nobody waits for right now: off the priority lane.
    pub bulk: bool,
}

impl Ops {
    /// Starts operation `code`; its arguments follow.
    pub fn op(&mut self, code: u32) -> &mut Encoder {
        self.count += 1;
        self.ops.u32(code)
    }

    /// Minor version 2, no tag.
    pub fn encode(&self, e: &mut Encoder, prefix: &Ops) {
        e.string("").u32(2).u32(prefix.count + self.count);
        e.opaque_fixed(prefix.ops.as_slice()).opaque_fixed(self.ops.as_slice());
    }
}

/// A COMPOUND's reply, read one operation at a time.
pub struct Results {
    d: Decoder,
    status: Status,
    left: u32,
}

impl Results {
    pub fn parse(mut d: Decoder) -> Result<Self> {
        let status = Status(d.u32()?);
        d.opaque()?;
        let left = d.u32()?;
        Ok(Self { d, status, left })
    }

    /// The status of the whole COMPOUND: that of the operation that failed, if one did.
    pub fn status(&self) -> Status {
        self.status
    }

    /// Results of operations with nothing to read, which must be these and successful.
    pub fn skip(&mut self, codes: &[u32]) -> Result<()> {
        codes.iter().try_for_each(|code| self.next(*code).map(drop))
    }

    /// The next result, which must be of operation `code` and successful.
    pub fn next(&mut self, code: u32) -> Result<&mut Decoder> {
        if self.left == 0 {
            return Err(Error::Nfs(self.status));
        }
        self.left -= 1;
        let got = self.d.u32()?;
        if got != code {
            return Err(Error::Other(format!(
                "result of operation {got} where {code} was expected"
            )));
        }
        match Status(self.d.u32()?) {
            Status::OK => Ok(&mut self.d),
            status => Err(Error::Nfs(status)),
        }
    }
}

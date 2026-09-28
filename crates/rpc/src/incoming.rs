//! What arrives on a connection: replies to our calls, and calls from the server (NFSv4.1
//! callbacks travel the other way on the same connection, RFC 8881 section 2.10.3), with the
//! replies we send to those.

use crate::Error;
use bytes::Bytes;
use nfs_xdr::{Decoder, Encoder};

pub struct Reply {
    pub xid: u32,
    /// The verifier's body (for RPC-with-TLS, "STARTTLS").
    pub verifier: Bytes,
    /// The results, still to be decoded, or why the call was not executed.
    pub result: Result<Decoder, Error>,
}

/// A call from the server.
pub struct Call {
    pub xid: u32,
    pub program: u32,
    pub version: u32,
    pub procedure: u32,
    pub args: Decoder,
}

pub enum Incoming {
    Reply(Reply),
    Call(Call),
}

/// Parses a record.
pub fn parse(record: Bytes) -> Result<Incoming, Error> {
    let mut d = Decoder::new(record);
    let xid = d.u32()?;
    if d.u32()? == 0 {
        d.u32()?;
        let (program, version, procedure) = (d.u32()?, d.u32()?, d.u32()?);
        for _ in 0..2 {
            d.u32()?;
            d.opaque()?;
        }
        return Ok(Incoming::Call(Call { xid, program, version, procedure, args: d }));
    }
    reply(xid, d).map(Incoming::Reply)
}

/// Our reply to a call from the server: its results, or PROG_UNAVAIL without any.
pub fn answer(xid: u32, results: Option<Bytes>) -> Bytes {
    let mut e = Encoder::new();
    e.u32(0).u32(xid).u32(1).u32(0).u32(0).u32(0);
    match results {
        Some(results) => e.u32(0).opaque_fixed(&results),
        None => e.u32(1),
    };
    let len = e.len() as u32 - 4;
    e.patch_u32(0, 0x8000_0000 | len);
    e.finish()
}

fn reply(xid: u32, mut d: Decoder) -> Result<Reply, Error> {
    if d.u32()? == 1 {
        let why = match d.u32()? {
            0 => format!("RPC version mismatch ({}-{})", d.u32()?, d.u32()?),
            _ => format!("authentication error {}", d.u32()?),
        };
        return Ok(Reply { xid, verifier: Bytes::new(), result: Err(Error::Rejected(why)) });
    }
    let _flavor = d.u32()?;
    let verifier = d.opaque()?;
    let result = match d.u32()? {
        0 => Ok(d),
        1 => Err(Error::Rejected("program unavailable".into())),
        2 => Err(Error::Rejected(format!("NFS version mismatch ({}-{})", d.u32()?, d.u32()?))),
        3 => Err(Error::Rejected("procedure unavailable".into())),
        4 => Err(Error::Rejected("garbage arguments".into())),
        other => Err(Error::Rejected(format!("system error {other}"))),
    };
    Ok(Reply { xid, verifier, result })
}

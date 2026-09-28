//! Record marking (RFC 5531 section 11): a record is fragments, each with a 4-byte header whose
//! top bit marks the last one.

use bytes::Bytes;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Largest record accepted: a 1 MiB READ reply with room to spare.
const MAX_RECORD: usize = 16 << 20;

/// Memory is taken as the data arrives, not as the header announces it: a server announcing 16 MiB
/// and sending nothing costs nothing (found by fuzzing).
pub async fn read(r: &mut (impl AsyncRead + Unpin)) -> io::Result<Bytes> {
    let mut record = Vec::new();
    loop {
        let mark = r.read_u32().await?;
        let len = (mark & 0x7fff_ffff) as usize;
        if record.len() + len > MAX_RECORD {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "RPC record too long"));
        }
        let start = record.len();
        record.reserve(len.min(1 << 20));
        (&mut *r).take(len as u64).read_to_end(&mut record).await?;
        if record.len() - start < len {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if mark & 0x8000_0000 != 0 {
            return Ok(Bytes::from(record));
        }
    }
}

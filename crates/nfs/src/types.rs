//! Values that travel in many operations.

use bytes::Bytes;
use nfs_xdr::{Decoder, Encoder};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A file handle: opaque, valid as long as the file exists.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fh(pub Bytes);

/// Open (or other) state on the server. The all-zeros stateid is the anonymous one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stateid {
    pub seqid: u32,
    pub other: [u8; 12],
}

impl Stateid {
    pub fn encode(&self, e: &mut Encoder) {
        e.u32(self.seqid).opaque_fixed(&self.other);
    }

    pub fn decode(d: &mut Decoder) -> nfs_xdr::Result<Self> {
        let seqid = d.u32()?;
        let other = d.opaque_fixed(12)?[..].try_into().unwrap_or_default();
        Ok(Self { seqid, other })
    }
}

/// nfstime4: seconds and nanoseconds since the epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time {
    pub secs: i64,
    pub nanos: u32,
}

impl Time {
    pub fn encode(&self, e: &mut Encoder) {
        e.i64(self.secs).u32(self.nanos);
    }

    pub fn decode(d: &mut Decoder) -> nfs_xdr::Result<Self> {
        Ok(Self { secs: d.i64()?, nanos: d.u32()? })
    }

    pub fn to_system(self) -> SystemTime {
        let nanos = Duration::from_nanos(u64::from(self.nanos));
        match u64::try_from(self.secs) {
            Ok(secs) => UNIX_EPOCH + Duration::from_secs(secs) + nanos,
            Err(_) => UNIX_EPOCH - Duration::from_secs(self.secs.unsigned_abs()) + nanos,
        }
    }

    pub fn from_system(time: SystemTime) -> Self {
        match time.duration_since(UNIX_EPOCH) {
            Ok(d) => Self { secs: d.as_secs() as i64, nanos: d.subsec_nanos() },
            Err(e) => {
                let d = e.duration();
                let (secs, nanos) = if d.subsec_nanos() == 0 {
                    (d.as_secs() as i64, 0)
                } else {
                    (d.as_secs() as i64 + 1, 1_000_000_000 - d.subsec_nanos())
                };
                Self { secs: -secs, nanos }
            }
        }
    }
}

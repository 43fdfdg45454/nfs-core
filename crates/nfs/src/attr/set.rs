use super::{MODE, SIZE, TIME_ACCESS_SET, TIME_MODIFY_SET, bitmap};
use crate::types::Time;
use nfs_xdr::Encoder;

/// A time to set: the server's clock, or a given one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetTime {
    ServerNow,
    To(Time),
}

/// Attributes to set (SETATTR, and a new file's CREATE or OPEN).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetAttrs {
    pub size: Option<u64>,
    pub mode: Option<u32>,
    pub accessed: Option<SetTime>,
    pub modified: Option<SetTime>,
}

impl SetAttrs {
    pub fn encode(&self, e: &mut Encoder) {
        let mut set = Vec::new();
        let mut v = Encoder::new();
        if let Some(size) = self.size {
            set.push(SIZE);
            v.u64(size);
        }
        if let Some(mode) = self.mode {
            set.push(MODE);
            v.u32(mode);
        }
        for (bit, time) in [(TIME_ACCESS_SET, self.accessed), (TIME_MODIFY_SET, self.modified)] {
            match time {
                None => continue,
                Some(SetTime::ServerNow) => v.u32(0),
                Some(SetTime::To(t)) => {
                    t.encode(v.u32(1));
                    &mut v
                }
            };
            set.push(bit);
        }
        e.bitmap(&bitmap(&set)).opaque(v.as_slice());
    }
}

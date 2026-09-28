use super::*;
use crate::types::Time;
use nfs_xdr::{Decoder, Error, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileType {
    #[default]
    Regular,
    Directory,
    Symlink,
    Other(u32),
}

/// Attributes as read; those not returned keep their default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attrs {
    pub kind: FileType,
    pub change: u64,
    pub size: u64,
    pub fileid: u64,
    pub mode: u32,
    pub links: u32,
    pub owner: String,
    pub group: String,
    pub used: u64,
    pub accessed: Time,
    pub changed: Time,
    pub modified: Time,
    /// Space for this client, free, and total, in bytes.
    pub space: (u64, u64, u64),
    pub lease_time: u32,
    pub max_read: u64,
    pub max_write: u64,
    /// In a listing, why this entry's attributes could not be read (an NFS status; 0 if they
    /// were): then only its name is known.
    pub error: u32,
}

impl Attrs {
    pub fn decode(d: &mut Decoder) -> Result<Self> {
        let words = d.bitmap()?;
        let mut v = Decoder::new(d.opaque()?);
        let mut a = Self::default();
        for bit in bits(&words) {
            match bit {
                TYPE => {
                    a.kind = match v.u32()? {
                        1 => FileType::Regular,
                        2 => FileType::Directory,
                        5 => FileType::Symlink,
                        n => FileType::Other(n),
                    }
                }
                CHANGE => a.change = v.u64()?,
                SIZE => a.size = v.u64()?,
                LEASE_TIME => a.lease_time = v.u32()?,
                RDATTR_ERROR => a.error = v.u32()?,
                FILEID => a.fileid = v.u64()?,
                MAXREAD => a.max_read = v.u64()?,
                MAXWRITE => a.max_write = v.u64()?,
                MODE => a.mode = v.u32()?,
                NUMLINKS => a.links = v.u32()?,
                OWNER => a.owner = v.string()?,
                OWNER_GROUP => a.group = v.string()?,
                SPACE_AVAIL => a.space.0 = v.u64()?,
                SPACE_FREE => a.space.1 = v.u64()?,
                SPACE_TOTAL => a.space.2 = v.u64()?,
                SPACE_USED => a.used = v.u64()?,
                TIME_ACCESS => a.accessed = Time::decode(&mut v)?,
                TIME_METADATA => a.changed = Time::decode(&mut v)?,
                TIME_MODIFY => a.modified = Time::decode(&mut v)?,
                _ => return Err(Error::Invalid),
            }
        }
        Ok(a)
    }
}

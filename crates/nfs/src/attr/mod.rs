//! File attributes (fattr4): the ones read and the ones set.

mod read;
mod set;

pub use read::{Attrs, FileType};
pub use set::{SetAttrs, SetTime};

pub const TYPE: u32 = 1;
pub const CHANGE: u32 = 3;
pub const SIZE: u32 = 4;
pub const LEASE_TIME: u32 = 10;
pub const RDATTR_ERROR: u32 = 11;
pub const FILEID: u32 = 20;
pub const MAXREAD: u32 = 30;
pub const MAXWRITE: u32 = 31;
pub const MODE: u32 = 33;
pub const NUMLINKS: u32 = 35;
pub const OWNER: u32 = 36;
pub const OWNER_GROUP: u32 = 37;
pub const SPACE_AVAIL: u32 = 42;
pub const SPACE_FREE: u32 = 43;
pub const SPACE_TOTAL: u32 = 44;
pub const SPACE_USED: u32 = 45;
pub const TIME_ACCESS: u32 = 47;
pub const TIME_ACCESS_SET: u32 = 48;
pub const TIME_METADATA: u32 = 52;
pub const TIME_MODIFY: u32 = 53;
pub const TIME_MODIFY_SET: u32 = 54;

/// What a file's GETATTR asks for.
pub const FILE: &[u32] = &[
    TYPE,
    CHANGE,
    SIZE,
    FILEID,
    MODE,
    NUMLINKS,
    OWNER,
    OWNER_GROUP,
    SPACE_USED,
    TIME_ACCESS,
    TIME_METADATA,
    TIME_MODIFY,
];

/// What a listing asks of each entry: a file's attributes, or why they cannot be read (an export
/// that requires other security, say) instead of failing the whole listing (NFS4ERR_WRONGSEC).
pub const ENTRY: &[u32] = &[
    TYPE,
    CHANGE,
    SIZE,
    RDATTR_ERROR,
    FILEID,
    MODE,
    NUMLINKS,
    OWNER,
    OWNER_GROUP,
    SPACE_USED,
    TIME_ACCESS,
    TIME_METADATA,
    TIME_MODIFY,
];

pub fn bitmap(bits: &[u32]) -> Vec<u32> {
    let mut words = vec![0u32; bits.iter().max().map_or(0, |b| *b as usize / 32 + 1)];
    bits.iter().for_each(|b| words[*b as usize / 32] |= 1 << (b % 32));
    words
}

pub(crate) fn bits(words: &[u32]) -> impl Iterator<Item = u32> + '_ {
    (0..words.len() as u32 * 32).filter(|b| words[*b as usize / 32] & (1 << (b % 32)) != 0)
}

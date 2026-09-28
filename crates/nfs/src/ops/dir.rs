//! Directories: listing, creating, removing, renaming, linking (RFC 8881 sections 18.23, 18.4,
//! 18.25, 18.26, 18.9, 18.24).

use super::*;
use crate::attr::{Attrs, SetAttrs};

/// An entry of a listing.
#[derive(Debug, Clone)]
pub struct Entry {
    pub cookie: u64,
    pub name: String,
    pub attrs: Attrs,
}

impl Ops {
    /// `cookie` and `verifier` are 0 for the first page, then those of the last entry and page.
    pub fn readdir(&mut self, cookie: u64, verifier: [u8; 8], bits: &[u32]) {
        let e = self.op(READDIR).u64(cookie).opaque_fixed(&verifier).u32(64 << 10).u32(256 << 10);
        e.bitmap(&crate::attr::bitmap(bits));
    }

    pub fn mkdir(&mut self, name: &str, attrs: &SetAttrs) {
        attrs.encode(self.op(CREATE).u32(2).string(name));
        self.modifies = true;
    }

    pub fn symlink(&mut self, name: &str, target: &str, attrs: &SetAttrs) {
        attrs.encode(self.op(CREATE).u32(5).string(target).string(name));
        self.modifies = true;
    }

    pub fn remove(&mut self, name: &str) {
        self.op(REMOVE).string(name);
        self.modifies = true;
    }

    /// From the saved file handle's directory to the current one's.
    pub fn rename(&mut self, from: &str, to: &str) {
        self.op(RENAME).string(from).string(to);
        self.modifies = true;
    }

    /// The saved file handle, linked as `name` in the current one.
    pub fn link(&mut self, name: &str) {
        self.op(LINK).string(name);
        self.modifies = true;
    }

    pub fn readlink(&mut self) {
        self.op(READLINK);
    }
}

/// The page's verifier, its entries, and whether it is the last page.
pub fn readdir(d: &mut Decoder) -> Result<([u8; 8], Vec<Entry>, bool)> {
    let verifier = d.opaque_fixed(8)?[..].try_into().unwrap_or_default();
    let mut entries = Vec::new();
    while d.bool()? {
        entries.push(Entry { cookie: d.u64()?, name: d.string()?, attrs: Attrs::decode(d)? });
    }
    Ok((verifier, entries, d.bool()?))
}

pub fn readlink(d: &mut Decoder) -> Result<String> {
    d.string()
}

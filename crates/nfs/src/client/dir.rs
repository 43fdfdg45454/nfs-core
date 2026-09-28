//! Directories and names: listing, creating, removing, renaming, linking, and attributes.

use super::Client;
use crate::attr::{self, Attrs, SetAttrs};
use crate::compound::Ops;
use crate::error::Result;
use crate::ops::dir::Entry;
use crate::ops::{
    self, CREATE, GETATTR, GETFH, LINK, PUTFH, READDIR, READLINK, REMOVE, RENAME, SAVEFH, SETATTR,
};
use crate::types::{Fh, Stateid};

impl Client {
    /// Every entry of `dir`, with its attributes (or, in `attrs.error`, why they cannot be read).
    pub async fn readdir(&self, dir: &Fh) -> Result<Vec<Entry>> {
        let (mut all, mut cookie, mut verifier) = (Vec::new(), 0, [0; 8]);
        loop {
            let mut ops = Ops::default();
            ops.putfh(dir).readdir(cookie, verifier, attr::ENTRY);
            let mut r = self.call(&ops).await?;
            r.next(PUTFH)?;
            let (v, entries, eof) = ops::dir::readdir(r.next(READDIR)?)?;
            verifier = v;
            cookie = entries.last().map_or(cookie, |e| e.cookie);
            let empty = entries.is_empty();
            all.extend(entries);
            if eof || empty {
                return Ok(all);
            }
        }
    }

    async fn create_in(&self, dir: &Fh, add: impl FnOnce(&mut Ops)) -> Result<(Fh, Attrs)> {
        let mut ops = Ops::default();
        ops.putfh(dir);
        add(&mut ops);
        ops.getfh().getattr(attr::FILE);
        let mut r = self.call(&ops).await?;
        r.next(PUTFH)?;
        let d = r.next(CREATE)?;
        ops::skip_change_info(d)?;
        d.bitmap()?;
        Ok((ops::fh(r.next(GETFH)?)?, Attrs::decode(r.next(GETATTR)?)?))
    }

    pub async fn mkdir(&self, dir: &Fh, name: &str, attrs: &SetAttrs) -> Result<(Fh, Attrs)> {
        let attrs = attrs.or_mode(self.session.mode(0o777));
        self.create_in(dir, |ops| ops.mkdir(name, &attrs)).await
    }

    pub async fn symlink(&self, dir: &Fh, name: &str, target: &str) -> Result<(Fh, Attrs)> {
        self.create_in(dir, |ops| ops.symlink(name, target, &SetAttrs::default())).await
    }

    pub async fn readlink(&self, link: &Fh) -> Result<String> {
        let mut ops = Ops::default();
        ops.putfh(link).readlink();
        let mut r = self.call(&ops).await?;
        r.next(PUTFH)?;
        Ok(ops::dir::readlink(r.next(READLINK)?)?)
    }

    pub async fn remove(&self, dir: &Fh, name: &str) -> Result<()> {
        let mut ops = Ops::default();
        ops.putfh(dir).remove(name);
        let mut r = self.call(&ops).await?;
        r.next(PUTFH)?;
        r.next(REMOVE).map(drop)
    }

    pub async fn rename(&self, from_dir: &Fh, from: &str, to_dir: &Fh, to: &str) -> Result<()> {
        let mut ops = Ops::default();
        ops.putfh(from_dir).savefh().putfh(to_dir).rename(from, to);
        let mut r = self.call(&ops).await?;
        r.skip(&[PUTFH, SAVEFH, PUTFH])?;
        r.next(RENAME).map(drop)
    }

    /// Another name for `file`, in `dir`.
    pub async fn link(&self, file: &Fh, dir: &Fh, name: &str) -> Result<()> {
        let mut ops = Ops::default();
        ops.putfh(file).savefh().putfh(dir).link(name);
        let mut r = self.call(&ops).await?;
        r.skip(&[PUTFH, SAVEFH, PUTFH])?;
        r.next(LINK).map(drop)
    }

    /// Without an open file (the anonymous stateid): mode, times, or a new size.
    pub async fn setattr(&self, fh: &Fh, attrs: &SetAttrs) -> Result<Attrs> {
        let mut ops = Ops::default();
        ops.putfh(fh).setattr(&Stateid::default(), attrs);
        ops.getattr(attr::FILE);
        let mut r = self.call(&ops).await?;
        r.next(PUTFH)?;
        r.next(SETATTR)?.bitmap()?;
        Ok(Attrs::decode(r.next(GETATTR)?)?)
    }
}

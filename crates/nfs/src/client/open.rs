//! Opening files: by name in a directory (creating it if asked), or by handle.

use super::{Client, File};
use crate::attr::{self, Attrs};
use crate::compound::Ops;
use crate::error::Result;
use crate::ops::open::{Claim, Create, Delegation, Opened};
use crate::ops::{self, GETATTR, GETFH, OPEN, PUTFH};
use crate::types::{Fh, Stateid};
use std::sync::Arc;

impl Client {
    /// Opens `name` in `dir`, creating it as `create` says.
    pub async fn create(
        self: &Arc<Self>,
        dir: &Fh,
        name: &str,
        create: Create,
        access: u32,
    ) -> Result<(File, Attrs)> {
        let owner = self.open_owner();
        let clientid = self.session.clientid().await;
        let mut ops = Ops::default();
        ops.putfh(dir).open(clientid, &owner, access, &create, Claim::Name(name));
        ops.getfh().getattr(attr::FILE);
        let mut r = self.call(&ops).await?;
        r.next(PUTFH)?;
        let opened = ops::open::open(r.next(OPEN)?)?;
        let fh = ops::fh(r.next(GETFH)?)?;
        let attrs = Attrs::decode(r.next(GETATTR)?)?;
        Ok((File::new(self, fh, access, owner, opened), attrs))
    }

    /// Opens a file by handle.
    pub async fn open(self: &Arc<Self>, fh: &Fh, access: u32) -> Result<File> {
        let file = File::new(self, fh.clone(), access, self.open_owner(), Default::default());
        file.reopen().await?;
        Ok(file)
    }

    /// Keeps a read delegation an OPEN gave, and gives back a write one (never asked for).
    pub(super) fn opened(&self, fh: &Fh, opened: &Opened) {
        let delegations = &self.session.callbacks.delegations;
        match opened.delegation {
            Some(Delegation::Read(stateid)) => delegations.granted(fh, stateid),
            Some(Delegation::Write(stateid)) => delegations.give_back(fh, stateid),
            None => {}
        }
    }

    /// The read delegation the client holds for `fh`: while it is held, nobody else changes the
    /// file.
    pub fn delegation(&self, fh: &Fh) -> Option<Stateid> {
        self.session.callbacks.delegations.get(fh)
    }
}

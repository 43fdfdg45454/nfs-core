//! Copies done by the server: CLONE shares the blocks (instant on a reflink file system), COPY
//! moves the data inside the server. Neither crosses the network.

use super::OpenFile;
use crate::compound::Ops;
use crate::error::Result;
use crate::ops::{self, CLONE, COPY, PUTFH, SAVEFH};

impl OpenFile {
    async fn server_copy(
        &self,
        to: &OpenFile,
        add: impl FnOnce(&mut Ops),
    ) -> Result<crate::compound::Results> {
        let mut ops = Ops::default();
        ops.putfh(&self.fh).savefh().putfh(&to.fh);
        add(&mut ops);
        let mut r = self.client.call(&ops).await?;
        r.skip(&[PUTFH, SAVEFH, PUTFH])?;
        Ok(r)
    }

    /// `count` bytes (0: to the end) from `offset` into `to` at `to_offset`; returns bytes copied.
    pub async fn copy_to(
        &self,
        to: &OpenFile,
        offset: u64,
        to_offset: u64,
        count: u64,
    ) -> Result<u64> {
        let (from, dest) = (self.stateid(), to.stateid());
        let mut r =
            self.server_copy(to, |ops| ops.copy(&from, &dest, offset, to_offset, count)).await?;
        Ok(ops::copy::copy(r.next(COPY)?)?)
    }

    pub async fn clone_to(
        &self,
        to: &OpenFile,
        offset: u64,
        to_offset: u64,
        count: u64,
    ) -> Result<()> {
        let (from, dest) = (self.stateid(), to.stateid());
        let mut r =
            self.server_copy(to, |ops| ops.clone(&from, &dest, offset, to_offset, count)).await?;
        r.next(CLONE).map(drop)
    }
}

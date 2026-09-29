//! Paths looked up in as few calls as the session allows, every step's handle and attributes
//! kept: a symbolic link on the way shows where it is, without a call per component.

use super::{Client, components};
use crate::attr::{self, Attrs};
use crate::compound::Ops;
use crate::error::{Error, Result};
use crate::ops::{self, GETATTR, GETFH, LOOKUP, PUTFH};
use crate::types::Fh;

/// SEQUENCE and PUTFH, then LOOKUP, GETFH and GETATTR for each step.
const STEP_OPS: usize = 3;

impl Client {
    /// Every step of `path` from `from` (the export's root if `None`), up to the first that
    /// fails, and that failure. The server does not follow symbolic links: past one, the next
    /// lookup fails, so the link is the last step returned.
    pub async fn walk(
        &self,
        from: Option<&Fh>,
        path: &str,
    ) -> Result<(Vec<(Fh, Attrs)>, Option<Error>)> {
        let names: Vec<&str> = components(path).collect();
        let per_call = (self.session.max_ops().await as usize).saturating_sub(2) / STEP_OPS;
        let mut steps: Vec<(Fh, Attrs)> = Vec::with_capacity(names.len());
        for chunk in names.chunks(per_call.max(1)) {
            let dir = steps.last().map_or(from.unwrap_or(&self.root), |(fh, _)| fh).clone();
            let mut ops = Ops::default();
            ops.putfh(&dir);
            chunk.iter().for_each(|name| _ = ops.lookup(name).getfh().getattr(attr::FILE));
            let mut r = self.call(&ops).await?;
            r.next(PUTFH)?;
            for _ in chunk {
                if let Err(error) = r.next(LOOKUP) {
                    return Ok((steps, Some(error)));
                }
                steps.push((ops::fh(r.next(GETFH)?)?, Attrs::decode(r.next(GETATTR)?)?));
            }
        }
        Ok((steps, None))
    }

    /// Follows `path` from `from` (the export's root if `None`).
    pub async fn lookup(&self, from: Option<&Fh>, path: &str) -> Result<(Fh, Attrs)> {
        let (mut steps, error) = self.walk(from, path).await?;
        match (error, steps.pop()) {
            (Some(error), _) => Err(error),
            (None, Some(last)) => Ok(last),
            (None, None) => {
                let fh = from.unwrap_or(&self.root).clone();
                let attrs = self.getattr(&fh).await?;
                Ok((fh, attrs))
            }
        }
    }
}

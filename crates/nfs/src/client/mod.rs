//! The client API: an export mounted over one session, and files and directories by handle.

mod copy;
mod dir;
mod file;
mod held;
mod lock;
mod open;
mod reclaim;
mod walk;

pub use file::{File, OpenFile};

use crate::attr::{self, Attrs};
use crate::compound::{Ops, Results};
use crate::config::Config;
use crate::error::Result;
use crate::ops::{self, GETATTR, GETFH, LOOKUP, PUTFH};
use crate::session::Session;
use crate::types::Fh;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct Client {
    session: Arc<Session>,
    root: Fh,
    max_io: u32,
    /// Numbers each open's owner: every File has its own open state.
    opens: AtomicU64,
}

fn components(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|c| !c.is_empty() && *c != ".")
}

impl Client {
    /// Sets up the session and finds `export` (its path under the server's NFSv4 root).
    pub async fn connect(config: Config, export: &str) -> Result<Arc<Self>> {
        let session = Session::new(config).await?;
        let mut ops = Ops::default();
        ops.putrootfh();
        components(export).for_each(|c| _ = ops.lookup(c));
        ops.getfh().getattr(&[attr::LEASE_TIME, attr::MAXREAD, attr::MAXWRITE]);
        let mut r = session.call(&ops).await?;
        r.next(ops::PUTROOTFH)?;
        for _ in components(export) {
            r.next(LOOKUP)?;
        }
        let root = ops::fh(r.next(GETFH)?)?;
        let fs = Attrs::decode(r.next(GETATTR)?)?;
        session.keep_lease(Duration::from_secs(u64::from(fs.lease_time.max(15))));
        let limit = |v: u64| {
            if v == 0 { u32::MAX } else { v.min(u64::from(u32::MAX)) as u32 }
        };
        let max_io = session.max_io().await.min(limit(fs.max_read)).min(limit(fs.max_write));
        Ok(Arc::new(Self { session, root, max_io, opens: 0.into() }))
    }

    pub fn root(&self) -> &Fh {
        &self.root
    }

    /// Largest READ or WRITE.
    pub fn max_io(&self) -> u32 {
        self.max_io
    }

    /// The client owner the server knows this client by.
    pub fn owner(&self) -> &str {
        self.session.owner()
    }

    /// How many times the server said it could not call this client back (a diagnostic).
    pub fn callbacks_down(&self) -> u32 {
        self.session.callbacks_down()
    }

    /// CB_NULL and CB_COMPOUND calls received, and SEQUENCE's last status flags (diagnosis).
    pub fn callback_counts(&self) -> [u32; 3] {
        self.session.callback_counts()
    }

    /// What the session's connections are doing now (for a live view).
    pub fn stats(&self) -> crate::Stats {
        self.session.stats()
    }

    /// Connections (or streams) in the session.
    pub fn connections(&self) -> usize {
        self.session.connections()
    }

    async fn call(&self, ops: &Ops) -> Result<Results> {
        self.session.call(ops).await
    }

    pub async fn getattr(&self, fh: &Fh) -> Result<Attrs> {
        let mut ops = Ops::default();
        ops.putfh(fh).getattr(attr::FILE);
        let mut r = self.call(&ops).await?;
        r.next(PUTFH)?;
        Ok(Attrs::decode(r.next(GETATTR)?)?)
    }

    /// Space for this client, free, and total, in bytes (quotas included).
    pub async fn space(&self) -> Result<(u64, u64, u64)> {
        let mut ops = Ops::default();
        ops.putfh(&self.root).getattr(&[attr::SPACE_AVAIL, attr::SPACE_FREE, attr::SPACE_TOTAL]);
        let mut r = self.call(&ops).await?;
        r.next(PUTFH)?;
        Ok(Attrs::decode(r.next(GETATTR)?)?.space)
    }

    fn open_owner(&self) -> Vec<u8> {
        format!("open-{}", self.opens.fetch_add(1, Ordering::Relaxed)).into_bytes()
    }

    /// The device changed networks (Wi-Fi and mobile data, a VPN coming up): connections are made
    /// anew now, on the new network, instead of when the old ones time out.
    pub async fn network_changed(&self) {
        self.session.network_changed().await;
    }

    pub fn close(&self) {
        self.session.close();
    }
}

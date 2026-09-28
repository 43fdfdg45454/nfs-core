//! The session's connections. Some are reserved for priority calls (metadata, and reads someone
//! waits for), so that their replies never queue behind bulk data (read-ahead, writes), which goes
//! to the others. Over QUIC one stream is enough: all share one congestion controller. Over TCP each
//! connection is a flow of its own, and one alone is too slow for readers that wait (stage 3):
//! a quarter of them, at least 2. A dead connection is replaced in the background, or right away
//! when none is left.

use crate::config::Config;
use crate::error::Result;
use crate::ops::session::SessionId;
use crate::transport;
use nfs_rpc::Connection;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub struct Channels {
    pub(super) config: Arc<Config>,
    pub(super) connections: Vec<Mutex<Option<Connection>>>,
    /// Answers the server's callbacks on every connection.
    handler: Arc<dyn nfs_rpc::Handler>,
    /// The session new connections are bound to, for callbacks too, once there is one.
    pub(super) session: std::sync::Mutex<Option<SessionId>>,
    /// A rebinding for callbacks is under way (or just was).
    pub(super) rebinding: std::sync::atomic::AtomicBool,
    /// How many times the server said it could not call back.
    pub rebinds: std::sync::atomic::AtomicU32,
    /// Connections made since the start (more than there are lanes: reconnections).
    pub(super) connects: std::sync::atomic::AtomicU32,
}

impl Channels {
    pub fn new(config: Arc<Config>, handler: Arc<dyn nfs_rpc::Handler>) -> Arc<Self> {
        let connections = (0..config.connections.max(1)).map(|_| Mutex::new(None)).collect();
        Arc::new(Self {
            config,
            connections,
            handler,
            session: Default::default(),
            rebinding: Default::default(),
            rebinds: Default::default(),
            connects: Default::default(),
        })
    }

    fn reserved(&self) -> usize {
        let quic = matches!(self.config.transport, crate::transport::Transport::Quic(_));
        reserved(self.connections.len(), quic)
    }

    /// The least busy live connection of the lanes for this kind of call; any live one if those
    /// are all gone.
    pub async fn get(self: &Arc<Self>, bulk: bool) -> Result<Connection> {
        let reserved = self.reserved();
        let lanes = if bulk { reserved..self.connections.len() } else { 0..reserved.max(1) };
        let (first, mut best) = (lanes.start, None::<Connection>);
        for pass in [lanes, 0..self.connections.len()] {
            for i in pass {
                let Ok(guard) = self.connections[i].try_lock() else { continue };
                match guard.as_ref().filter(|c| !c.is_closed()) {
                    Some(c) if best.as_ref().is_none_or(|b| c.pending() < b.pending()) => {
                        best = Some(c.clone())
                    }
                    Some(_) => {}
                    None => {
                        drop(guard);
                        tokio::spawn(self.clone().revive(i));
                    }
                }
            }
            if let Some(connection) = best {
                return Ok(connection);
            }
        }
        self.connect(first).await
    }

    /// Connects every missing connection at once: done when the session starts, so that none is
    /// opened in the middle of a transfer.
    pub async fn warm(self: &Arc<Self>) -> Result<()> {
        let tasks: Vec<_> =
            (0..self.connections.len()).map(|i| tokio::spawn(self.clone().revive(i))).collect();
        for task in tasks {
            _ = task.await;
        }
        self.get(false).await.map(drop)
    }

    async fn connect(&self, i: usize) -> Result<Connection> {
        let mut slot = self.connections[i].lock().await;
        if let Some(c) = slot.as_ref().filter(|c| !c.is_closed()) {
            return Ok(c.clone());
        }
        let c = transport::connect(&self.config).await?;
        c.serve(self.handler.clone());
        let session = *self.session.lock().expect("not poisoned");
        if let Some(id) = session {
            super::bind::bind(&c, &self.config, &id).await;
        }
        *slot = Some(c.clone());
        self.connects.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(c)
    }

    /// Connections up now, and the calls waiting on them (one being connected counts as down).
    pub fn load(&self) -> (usize, usize) {
        let up = self.connections.iter().filter_map(|slot| {
            slot.try_lock().ok()?.as_ref().filter(|c| !c.is_closed()).map(|c| c.pending())
        });
        up.fold((0, 0), |(alive, pending), p| (alive + 1, pending + p))
    }

    /// Replaces connection `i` if it is gone; after a failure, waits a second so a server that is
    /// away is not hammered.
    async fn revive(self: Arc<Self>, i: usize) {
        let Ok(guard) = self.connections[i].try_lock() else {
            return;
        };
        drop(guard);
        if self.connect(i).await.is_err() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// The network changed: every connection (and the QUIC one under them) goes, and the next
    /// calls connect anew on the new network instead of waiting for the old ones to time out.
    pub async fn restart(&self) {
        if let crate::transport::Transport::Quic(tunnel) = &self.config.transport {
            tunnel.reset().await;
        }
        self.close();
    }

    pub fn close(&self) {
        for slot in &self.connections {
            if let Some(c) = slot.try_lock().ok().and_then(|g| g.clone()) {
                c.close();
            }
        }
    }
}

/// Lanes kept for what a reader waits for: one stream over QUIC (they share one congestion
/// controller); over TCP a quarter, at least 2; always one left for the bulk.
fn reserved(lanes: usize, quic: bool) -> usize {
    match lanes {
        0 | 1 => 0,
        _ if quic => 1,
        _ => (lanes / 4).max(2).min(lanes - 1),
    }
}

#[cfg(test)]
#[test]
fn every_lane_count_keeps_urgent_and_bulk_lanes() {
    let tcp: Vec<_> = [1, 2, 3, 4, 8, 16, 64].map(|lanes| reserved(lanes, false)).into();
    assert_eq!(tcp, [0, 1, 2, 2, 2, 4, 16]);
    assert_eq!((reserved(1, true), reserved(4, true)), (0, 1));
}

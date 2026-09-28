//! Binding connections to the session for both directions (BIND_CONN_TO_SESSION), so that any of
//! them can carry the server's callbacks: nfsd uses another when the one it used goes away.

use super::channels::Channels;
use crate::compound::{self, Ops, Results};
use crate::config::Config;
use crate::ops::{self, session::SessionId};
use nfs_rpc::Connection;
use std::sync::Arc;

/// Best effort: a connection left unbound still carries calls (the server binds it to the fore
/// channel on its first SEQUENCE).
pub async fn bind(connection: &Connection, config: &Config, id: &SessionId) {
    let mut ops = Ops::default();
    ops.bind_conn_to_session(id);
    let reply =
        connection.call(compound::PROCEDURE, &config.auth, |e| ops.encode(e, &Ops::default()));
    if let Ok(reply) = reply.await {
        _ = Results::parse(reply).and_then(|mut r| r.next(ops::BIND_CONN_TO_SESSION).map(drop));
    }
}

impl Channels {
    /// The server says it cannot call back: `connection` (which just answered) is bound for both
    /// directions again, which also makes the server check the path anew. At most every 2 s.
    pub fn rebind(self: &Arc<Self>, connection: &Connection) {
        self.rebinds.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.rebinding.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let (channels, connection) = (self.clone(), connection.clone());
        tokio::spawn(async move {
            let id = *channels.session.lock().expect("not poisoned");
            if let Some(id) = id {
                bind(&connection, &channels.config, &id).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            channels.rebinding.store(false, std::sync::atomic::Ordering::Relaxed);
        });
    }

    /// A new session: the live connections and every later one are bound to it.
    pub fn bind_all(self: &Arc<Self>, id: SessionId) {
        *self.session.lock().expect("not poisoned") = Some(id);
        for slot in &self.connections {
            if let Some(c) = slot.try_lock().ok().and_then(|g| g.clone()) {
                let channels = self.clone();
                tokio::spawn(async move { bind(&c, &channels.config, &id).await });
            }
        }
    }
}

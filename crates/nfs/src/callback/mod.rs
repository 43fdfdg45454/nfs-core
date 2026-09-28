//! The client's side of the back channel (RFC 8881 section 20): the server's CB_COMPOUNDs,
//! through CB_SEQUENCE's slots, recalling delegations and telling of locks that may be free.

mod compound;
mod delegations;

pub use delegations::Delegations;

use crate::ops::session::{CALLBACK_PROGRAM, SessionId};
use crate::types::Fh;
use bytes::Bytes;
use nfs_rpc::Decoder;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// Back-channel slots asked for in CREATE_SESSION.
pub const SLOTS: u32 = 4;

pub struct Callbacks {
    /// The session callbacks come through; replaced when the client recovers.
    session: Mutex<SessionId>,
    /// Each slot's last sequence id and reply, for a replay.
    slots: Mutex<HashMap<u32, (u32, Bytes)>>,
    pub delegations: Delegations,
    /// Who waits for a lock on each file (CB_NOTIFY_LOCK wakes them all to try again).
    lock_waits: Mutex<HashMap<Fh, Arc<Notify>>>,
    /// CB_NULL and CB_COMPOUND calls received, and SEQUENCE's last status flags (diagnosis).
    pub(crate) seen: [std::sync::atomic::AtomicU32; 3],
}

impl Callbacks {
    pub fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<(Fh, crate::Stateid)>) {
        let (delegations, returns) = Delegations::new();
        let callbacks = Self {
            session: Mutex::default(),
            slots: Mutex::default(),
            delegations,
            lock_waits: Mutex::default(),
            seen: Default::default(),
        };
        (Arc::new(callbacks), returns)
    }

    pub fn set_session(&self, id: SessionId) {
        *self.session.lock().expect("not poisoned") = id;
        self.slots.lock().expect("not poisoned").clear();
    }

    /// What wakes a waiter for a lock on `fh`.
    pub fn lock_wait(&self, fh: &Fh) -> Arc<Notify> {
        self.lock_waits.lock().expect("not poisoned").entry(fh.clone()).or_default().clone()
    }

    fn lock_may_be_free(&self, fh: &Fh) {
        if let Some(notify) = self.lock_waits.lock().expect("not poisoned").get(fh) {
            notify.notify_waiters();
        }
    }
}

impl nfs_rpc::Handler for Callbacks {
    fn call(&self, program: u32, version: u32, procedure: u32, args: Decoder) -> Option<Bytes> {
        let seen = |i: usize| self.seen[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match (program, version, procedure) {
            (CALLBACK_PROGRAM, 1, 0) => Some(Bytes::new()).inspect(|_| _ = seen(0)),
            (CALLBACK_PROGRAM, 1, 1) => Some(self.compound(args)).inspect(|_| _ = seen(1)),
            _ => None,
        }
    }
}

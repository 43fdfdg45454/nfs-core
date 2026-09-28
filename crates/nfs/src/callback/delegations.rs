//! Read delegations the client holds, by file, and their return when the server recalls them
//! (or when one was granted that was not asked for).

use crate::types::{Fh, Stateid};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tokio::sync::mpsc;

pub struct Delegations {
    held: Mutex<HashMap<Fh, Stateid>>,
    /// Recalled before the OPEN that granted them was seen: not to be recorded then.
    recalled: Mutex<HashSet<[u8; 12]>>,
    /// DELEGRETURNs to send, by the client's returner task.
    returns: mpsc::UnboundedSender<(Fh, Stateid)>,
}

impl Delegations {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<(Fh, Stateid)>) {
        let (returns, receiver) = mpsc::unbounded_channel();
        let held = Mutex::default();
        (Self { held, recalled: Mutex::default(), returns }, receiver)
    }

    /// A read delegation granted by an OPEN.
    pub fn granted(&self, fh: &Fh, stateid: Stateid) {
        if self.recalled.lock().expect("not poisoned").remove(&stateid.other) {
            return;
        }
        self.held.lock().expect("not poisoned").insert(fh.clone(), stateid);
    }

    pub fn get(&self, fh: &Fh) -> Option<Stateid> {
        self.held.lock().expect("not poisoned").get(fh).copied()
    }

    /// Given back: recalled by the server (CB_RECALL), or not wanted.
    pub fn give_back(&self, fh: &Fh, stateid: Stateid) {
        let mut held = self.held.lock().expect("not poisoned");
        if held.get(fh).is_some_and(|s| s.other == stateid.other) {
            held.remove(fh);
        } else {
            self.recalled.lock().expect("not poisoned").insert(stateid.other);
        }
        _ = self.returns.send((fh.clone(), stateid));
    }

    /// All of them (CB_RECALL_ANY).
    pub fn give_back_all(&self) {
        let all: Vec<_> = self.held.lock().expect("not poisoned").drain().collect();
        all.into_iter().for_each(|(fh, stateid)| _ = self.returns.send((fh, stateid)));
    }

    /// The server forgot them with the client id.
    pub fn forget(&self) {
        self.held.lock().expect("not poisoned").clear();
    }
}

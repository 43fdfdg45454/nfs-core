//! The session's slot table (RFC 8881 section 2.10.6): one request per slot at a time, each with
//! a sequence id the server uses to recognize a replay.

use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub struct Slots {
    /// Free slots and their next sequence id, lowest slot first.
    free: Mutex<Vec<(u32, u32)>>,
    permits: Arc<Semaphore>,
    count: u32,
}

impl Slots {
    pub fn new(count: u32) -> Arc<Self> {
        let free = (0..count).rev().map(|id| (id, 1)).collect();
        Arc::new(Self {
            free: Mutex::new(free),
            permits: Arc::new(Semaphore::new(count as usize)),
            count,
        })
    }

    pub fn highest(&self) -> u32 {
        self.count - 1
    }

    pub async fn acquire(self: &Arc<Self>) -> Slot {
        let permit = self.permits.clone().acquire_owned().await.expect("never closed");
        let (id, seqid) =
            self.free.lock().expect("not poisoned").pop().expect("a permit means a free slot");
        Slot { slots: self.clone(), id, seqid, permit: Some(permit) }
    }
}

pub struct Slot {
    slots: Arc<Slots>,
    pub id: u32,
    pub seqid: u32,
    permit: Option<OwnedSemaphorePermit>,
}

impl Slot {
    /// The server executed (or answered from its cache) the request on this slot.
    pub fn advance(&mut self) {
        self.seqid = self.seqid.wrapping_add(1);
    }

    /// Whether the server ran the last request is unknown: the slot is not used again in this
    /// session (the next session starts over).
    pub fn retire(mut self) {
        if let Some(permit) = self.permit.take() {
            permit.forget();
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        // The slot goes back before its permit: whoever gets the permit finds it.
        if let Some(permit) = self.permit.take() {
            let mut free = self.slots.free.lock().expect("not poisoned");
            free.push((self.id, self.seqid));
            free.sort_by_key(|slot| std::cmp::Reverse(slot.0));
            drop(free);
            drop(permit);
        }
    }
}

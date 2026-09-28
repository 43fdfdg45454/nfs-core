//! The read-ahead queue shared by all files: a fixed number of READs in flight, the blocks
//! nearest to their reader first. A block nobody wants any more (the reader jumped) is dropped
//! when its turn comes.

use crate::shared::Shared;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::Semaphore;

struct Job {
    /// Bytes between the reader and the block; then the order of arrival.
    distance: u64,
    seq: u64,
    shared: Weak<Shared>,
    block: u64,
}

impl Ord for Job {
    fn cmp(&self, other: &Self) -> Ordering {
        (other.distance, other.seq).cmp(&(self.distance, self.seq))
    }
}

impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Job {}

pub struct Queue {
    /// Readers waiting for the network right now: while any does, uploads step aside.
    pub(crate) waiting: std::sync::atomic::AtomicUsize,
    jobs: Mutex<BinaryHeap<Job>>,
    /// One permit per queued job.
    queued: Semaphore,
    seq: AtomicU64,
}

impl Queue {
    pub fn start(workers: usize) -> Arc<Self> {
        let queue = Arc::new(Self {
            waiting: 0.into(),
            jobs: Mutex::default(),
            queued: Semaphore::new(0),
            seq: 0.into(),
        });
        for _ in 0..workers.max(1) {
            tokio::spawn(work(Arc::downgrade(&queue)));
        }
        queue
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.lock().expect("not poisoned").is_empty()
    }

    pub fn push(&self, shared: &Arc<Shared>, block: u64, distance: u64) {
        let seq = self.seq.fetch_add(1, Relaxed);
        let job = Job { distance, seq, shared: Arc::downgrade(shared), block };
        self.jobs.lock().expect("not poisoned").push(job);
        self.queued.add_permits(1);
    }
}

async fn work(queue: Weak<Queue>) {
    loop {
        let job = {
            let Some(queue) = queue.upgrade() else { return };
            let Ok(permit) = queue.queued.acquire().await else { return };
            permit.forget();
            queue.jobs.lock().expect("not poisoned").pop()
        };
        if let Some(shared) = job.and_then(|job| Some((job.shared.upgrade()?, job.block))) {
            shared.0.fetch_ahead(shared.1).await;
        }
    }
}

//! One COMPOUND through the session: a slot, SEQUENCE first, and what to do when the connection
//! drops (replay on the same slot: the server answers from its reply cache), when the server is
//! busy, or when it forgot the session.

use super::attempt::Attempt;
use super::{Session, State};
use crate::compound::{Ops, Results};
use crate::error::{Error, Result};
use crate::ops;
use crate::status::Status;
use std::sync::Arc;
use std::time::{Duration, Instant};

impl Session {
    /// Sends `ops` after a SEQUENCE, retrying until the configured call timeout.
    pub async fn call(&self, ops: &Ops) -> Result<Results> {
        let deadline = Instant::now() + self.config.call_timeout;
        let mut pause = Duration::from_millis(100);
        loop {
            let state = self.state.read().await.clone();
            let mut slot = state.slots.acquire().await;
            match self.attempt(&state, &mut slot, ops, deadline).await {
                Attempt::Done(result) => {
                    self.touch();
                    return result;
                }
                Attempt::Unknown(error) => {
                    slot.retire();
                    return Err(error);
                }
                Attempt::LostSession => {
                    drop(slot);
                    self.recover(state.generation).await?;
                    continue;
                }
                Attempt::Busy => drop(slot),
            }
            if Instant::now() >= deadline {
                return Err(Error::Nfs(Status::DELAY));
            }
            tokio::time::sleep(pause).await;
            pause = (pause * 2).min(Duration::from_secs(2));
        }
    }

    /// RECLAIM_COMPLETE, once per new client id; without recovery, which calls it.
    pub(super) async fn reclaim_complete(&self) -> Result<()> {
        let state: Arc<State> = self.state.read().await.clone();
        let mut ops = Ops::default();
        ops.reclaim_complete();
        let mut slot = state.slots.acquire().await;
        let deadline = Instant::now() + self.config.call_timeout;
        match self.attempt(&state, &mut slot, &ops, deadline).await {
            Attempt::Done(Ok(mut results)) => match results.next(ops::RECLAIM_COMPLETE) {
                Ok(_) | Err(Error::Nfs(Status::COMPLETE_ALREADY)) => Ok(()),
                Err(e) => Err(e),
            },
            Attempt::Done(Err(e)) | Attempt::Unknown(e) => Err(e),
            Attempt::Busy | Attempt::LostSession => {
                Err(Error::Other("the new session failed at once".into()))
            }
        }
    }
}

//! One try of a COMPOUND on a slot: replays on the same slot and sequence id after a lost
//! connection (the server answers from its reply cache), and says what the caller must do next.

use super::slots::Slot;
use super::{Session, State};
use crate::compound::{self, Ops, Results};
use crate::error::{Error, Result};
use crate::ops;
use crate::status::Status;
use std::time::{Duration, Instant};

/// SEQ4_STATUS_CB_PATH_DOWN, _CB_PATH_DOWN_SESSION and _BACKCHANNEL_FAULT: the server cannot call
/// the client back (RFC 8881 section 18.46.3).
const CALLBACKS_DOWN: u32 = 0x1 | 0x200 | 0x400;

pub(super) enum Attempt {
    Done(Result<Results>),
    /// The call may or may not have run: its slot cannot be trusted again.
    Unknown(Error),
    /// NFS4ERR_DELAY or NFS4ERR_GRACE from an operation: not run, send it again later.
    Busy,
    LostSession,
}

impl Session {
    pub(super) async fn attempt(
        &self,
        state: &State,
        slot: &mut Slot,
        ops: &Ops,
        deadline: Instant,
    ) -> Attempt {
        loop {
            let late = Instant::now() >= deadline;
            let connection = match self.channels.get(ops.bulk).await {
                Ok(c) => c,
                Err(e) if late => return Attempt::Unknown(e),
                Err(_) => {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            let mut sequence = Ops::default();
            sequence.sequence(&state.id, slot.seqid, slot.id, state.slots.highest(), ops.modifies);
            let reply = connection
                .call(compound::PROCEDURE, &self.config.auth, |e| ops.encode(e, &sequence))
                .await;
            let mut results = match reply {
                Err(nfs_rpc::Error::Disconnected(_) | nfs_rpc::Error::Stalled) if !late => {
                    connection.close();
                    continue;
                }
                Err(e @ (nfs_rpc::Error::Disconnected(_) | nfs_rpc::Error::Stalled)) => {
                    return Attempt::Unknown(e.into());
                }
                Err(e) => return Attempt::Done(Err(e.into())),
                Ok(d) => match Results::parse(d) {
                    Ok(results) => results,
                    Err(e) => return Attempt::Unknown(e),
                },
            };
            match results.next(ops::SEQUENCE).map(ops::session::sequence) {
                Ok(Ok((_, flags))) => {
                    slot.advance();
                    self.callbacks.seen[2].store(flags, std::sync::atomic::Ordering::Relaxed);
                    if flags & CALLBACKS_DOWN != 0 {
                        self.channels.rebind(&connection);
                    }
                    match results.status() {
                        Status::DELAY | Status::GRACE => return Attempt::Busy,
                        // A replay whose reply the server did not keep (nfsd answers so in the
                        // operation after SEQUENCE when it cached SEQUENCE's reply alone): what
                        // may run twice goes again with the slot's next sequence id.
                        Status::RETRY_UNCACHED_REP if !ops.modifies => {}
                        Status::RETRY_UNCACHED_REP => return Attempt::Done(Err(Error::Uncertain)),
                        _ => return Attempt::Done(Ok(results)),
                    }
                }
                // The server lost the reply of what it ran: a read runs again, a change is unknown.
                Ok(Err(e)) => return Attempt::Unknown(e.into()),
                Err(Error::Nfs(Status::RETRY_UNCACHED_REP)) if !ops.modifies => slot.advance(),
                Err(Error::Nfs(Status::RETRY_UNCACHED_REP)) => {
                    slot.advance();
                    return Attempt::Done(Err(Error::Uncertain));
                }
                // SEQUENCE itself busy: nothing ran, the same slot and sequence id go again.
                Err(Error::Nfs(Status::DELAY)) if !late => {
                    tokio::time::sleep(Duration::from_millis(200)).await
                }
                Err(Error::Nfs(status)) if status.lost_session() => return Attempt::LostSession,
                Err(e) => return Attempt::Done(Err(e)),
            }
        }
    }
}

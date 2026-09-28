//! CB_COMPOUND: CB_SEQUENCE first, then the operations, until one fails.

use super::{Callbacks, SLOTS};
use crate::status::Status;
use crate::types::{Fh, Stateid};
use bytes::Bytes;
use nfs_xdr::{Decoder, Encoder, Result};

const CB_GETATTR: u32 = 3;
const CB_RECALL: u32 = 4;
const CB_RECALL_ANY: u32 = 8;
const CB_RECALL_SLOT: u32 = 10;
const CB_SEQUENCE: u32 = 11;
const CB_NOTIFY_LOCK: u32 = 13;
const CB_ILLEGAL: u32 = 10044;

/// Where a CB_SEQUENCE leaves the rest: go on (with the slot and sequence id to cache the reply
/// under), or answer with a reply cached before.
enum Sequenced {
    Go(u32, u32),
    Replay(Bytes),
}

impl Callbacks {
    pub(super) fn compound(&self, mut d: Decoder) -> Bytes {
        let (tag, mut results, mut count) = (d.opaque().unwrap_or_default(), Encoder::new(), 0);
        let mut status = Status::OK;
        let mut slot = None;
        let ops = d.u32().and_then(|_| d.u32()).and_then(|_| d.u32()).unwrap_or(0);
        for i in 0..ops {
            let code = d.u32().unwrap_or(CB_ILLEGAL);
            let mut body = Encoder::new();
            let result = match (i, code) {
                (0, CB_SEQUENCE) => match self.sequence(&mut d, &mut body) {
                    Ok(Ok(Sequenced::Replay(reply))) => return reply,
                    Ok(Ok(Sequenced::Go(id, seqid))) => {
                        slot = Some((id, seqid));
                        Ok(Status::OK)
                    }
                    Ok(Err(status)) => Ok(status),
                    Err(e) => Err(e),
                },
                (0, _) => Ok(Status::OP_NOT_IN_SESSION),
                (_, CB_SEQUENCE) => Ok(Status::SEQUENCE_POS),
                _ => self.operation(code, &mut d),
            };
            status = result.unwrap_or(Status::BADXDR);
            let code = if (3..=15).contains(&code) { code } else { CB_ILLEGAL };
            results.u32(code).u32(status.0).opaque_fixed(body.as_slice());
            count += 1;
            if status != Status::OK {
                break;
            }
        }
        let mut reply = Encoder::new();
        reply.u32(status.0).opaque(&tag).u32(count).opaque_fixed(results.as_slice());
        let reply = reply.finish();
        if let Some((id, seqid)) = slot {
            self.slots.lock().expect("not poisoned").insert(id, (seqid, reply.clone()));
        }
        reply
    }

    fn sequence(
        &self,
        d: &mut Decoder,
        out: &mut Encoder,
    ) -> Result<std::result::Result<Sequenced, Status>> {
        let session = d.opaque_fixed(16)?;
        let (seqid, id, _highest, _cache) = (d.u32()?, d.u32()?, d.u32()?, d.bool()?);
        for _ in 0..d.u32()? {
            d.opaque_fixed(16)?;
            (0..d.u32()?).try_for_each(|_| d.u32().and_then(|_| d.u32()).map(drop))?;
        }
        if session[..] != self.session.lock().expect("not poisoned")[..] {
            return Ok(Err(Status::BADSESSION));
        }
        if id >= SLOTS {
            return Ok(Err(Status::BADSLOT));
        }
        match self.slots.lock().expect("not poisoned").get(&id) {
            Some((last, reply)) if *last == seqid => {
                return Ok(Ok(Sequenced::Replay(reply.clone())));
            }
            Some((last, _)) if last.wrapping_add(1) != seqid => {
                return Ok(Err(Status::SEQ_MISORDERED));
            }
            _ => {}
        }
        out.opaque_fixed(&session).u32(seqid).u32(id).u32(SLOTS - 1).u32(SLOTS - 1);
        Ok(Ok(Sequenced::Go(id, seqid)))
    }

    fn operation(&self, code: u32, d: &mut Decoder) -> Result<Status> {
        Ok(match code {
            CB_RECALL => {
                let stateid = Stateid::decode(d)?;
                d.bool()?;
                self.delegations.give_back(&Fh(d.opaque()?), stateid);
                Status::OK
            }
            CB_RECALL_ANY => {
                d.u32()?;
                d.bitmap()?;
                self.delegations.give_back_all();
                Status::OK
            }
            CB_NOTIFY_LOCK => {
                let fh = Fh(d.opaque()?);
                d.u64()?;
                d.opaque()?;
                self.lock_may_be_free(&fh);
                Status::OK
            }
            // Only write delegations are asked for attributes, and none is ever kept.
            CB_GETATTR => Status::BADHANDLE,
            CB_RECALL_SLOT => {
                d.u32()?;
                Status::OK
            }
            // Layouts, directory notices, pushed delegations, devices, asynchronous copies.
            5 | 6 | 7 | 9 | 12 | 14 | 15 => Status::NOTSUPP,
            _ => Status::OP_ILLEGAL,
        })
    }
}

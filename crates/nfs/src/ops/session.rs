//! Client and session setup (RFC 8881 sections 18.35, 18.36, 18.46, 18.51).

use super::*;
use nfs_xdr::Encoder;

/// EXCHGID4_FLAG_USE_NON_PNFS: a plain NFSv4.1+ client.
const USE_NON_PNFS: u32 = 0x0001_0000;
/// CREATE_SESSION4_FLAG_CONN_BACK_CHAN: the connection also carries the server's callbacks.
const CONN_BACK_CHAN: u32 = 0x2;
/// The RPC program the server calls back (any number the client picks).
pub const CALLBACK_PROGRAM: u32 = 0x4000_0000;
/// CDFC4_FORE_OR_BOTH: a connection bound for both directions if the server agrees.
const FORE_OR_BOTH: u32 = 3;

pub type SessionId = [u8; 16];

/// What the client asks of the fore channel; the server answers with what it grants.
#[derive(Debug, Clone, Copy)]
pub struct Channel {
    pub max_request: u32,
    pub max_response: u32,
    pub max_response_cached: u32,
    pub max_ops: u32,
    pub slots: u32,
}

impl Channel {
    fn encode(&self, e: &mut Encoder) {
        e.u32(0).u32(self.max_request).u32(self.max_response).u32(self.max_response_cached);
        e.u32(self.max_ops).u32(self.slots).u32(0);
    }

    fn decode(d: &mut Decoder) -> Result<Self> {
        d.u32()?;
        let channel = Self {
            max_request: d.u32()?,
            max_response: d.u32()?,
            max_response_cached: d.u32()?,
            max_ops: d.u32()?,
            slots: d.u32()?,
        };
        (0..d.u32()?).try_for_each(|_| d.u32().map(drop))?;
        Ok(channel)
    }
}

impl Ops {
    /// `owner` names this client for good; `verifier` changes each time it starts.
    pub fn exchange_id(&mut self, verifier: [u8; 8], owner: &[u8]) {
        self.op(EXCHANGE_ID).opaque_fixed(&verifier).opaque(owner).u32(USE_NON_PNFS).u32(0).u32(0);
    }

    /// With a back channel (callbacks with AUTH_NONE): small calls, few at a time.
    pub fn create_session(&mut self, clientid: u64, sequence: u32, fore: Channel) {
        let e = self.op(CREATE_SESSION).u64(clientid).u32(sequence).u32(CONN_BACK_CHAN);
        fore.encode(e);
        Channel {
            max_request: 16 << 10,
            max_response: 16 << 10,
            max_response_cached: 0,
            max_ops: 16,
            slots: 4,
        }
        .encode(e);
        e.u32(CALLBACK_PROGRAM).u32(1).u32(0);
    }

    /// Binds the connection it is sent on to the session, for callbacks too (RFC 8881 section
    /// 18.34): alone in its COMPOUND.
    pub fn bind_conn_to_session(&mut self, session: &SessionId) {
        self.op(BIND_CONN_TO_SESSION).opaque_fixed(session).u32(FORE_OR_BOTH).bool(false);
    }

    pub fn sequence(
        &mut self,
        session: &SessionId,
        seqid: u32,
        slot: u32,
        highest: u32,
        cache: bool,
    ) {
        self.op(SEQUENCE).opaque_fixed(session).u32(seqid).u32(slot).u32(highest).bool(cache);
    }

    pub fn reclaim_complete(&mut self) {
        self.op(RECLAIM_COMPLETE).bool(false);
    }
}

/// Client id and the sequence CREATE_SESSION must use.
pub fn exchange_id(d: &mut Decoder) -> Result<(u64, u32)> {
    let (clientid, sequence) = (d.u64()?, d.u32()?);
    d.u32()?;
    if d.u32()? != 0 {
        return Err(nfs_xdr::Error::Invalid);
    }
    Ok((clientid, sequence))
}

pub fn create_session(d: &mut Decoder) -> Result<(SessionId, Channel)> {
    let id = d.opaque_fixed(16)?[..].try_into().unwrap_or_default();
    d.u32()?;
    d.u32()?;
    Ok((id, Channel::decode(d)?))
}

/// The slot count the server wants now, and the status flags (RFC 8881 section 18.46.3).
pub fn sequence(d: &mut Decoder) -> Result<(u32, u32)> {
    d.opaque_fixed(16)?;
    d.u32()?;
    d.u32()?;
    d.u32()?;
    Ok((d.u32()?, d.u32()?))
}

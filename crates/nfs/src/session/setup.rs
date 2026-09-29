//! A client id and a session (RFC 8881 sections 18.35, 18.36), set up on one connection outside
//! any session.

use crate::compound::{self, Ops, Results};
use crate::config::Config;
use crate::error::Result;
use crate::ops::{self, session::Channel, session::SessionId};
use nfs_rpc::Connection;

/// What the client asks for: 1 MiB transfers with room for the headers, and many requests in
/// flight (the server may grant fewer).
const FORE: Channel = Channel {
    max_request: (1 << 20) + (64 << 10),
    max_response: (1 << 20) + (64 << 10),
    max_response_cached: 64 << 10,
    // A path of some 20 components looked up in one call (walk.rs); the server may grant fewer.
    max_ops: 64,
    slots: 64,
};

async fn call(connection: &Connection, config: &Config, ops: Ops) -> Result<Results> {
    let reply = connection
        .call(compound::PROCEDURE, &config.auth, |e| ops.encode(e, &Ops::default()))
        .await?;
    Results::parse(reply)
}

pub struct Established {
    pub clientid: u64,
    pub id: SessionId,
    pub fore: Channel,
}

pub async fn establish(
    connection: &Connection,
    config: &Config,
    verifier: [u8; 8],
) -> Result<Established> {
    let mut exchange = Ops::default();
    exchange.exchange_id(verifier, config.owner.as_bytes());
    let (clientid, sequence) = ops::session::exchange_id(
        call(connection, config, exchange).await?.next(ops::EXCHANGE_ID)?,
    )?;
    let mut create = Ops::default();
    create.create_session(clientid, sequence, FORE);
    let (id, fore) = ops::session::create_session(
        call(connection, config, create).await?.next(ops::CREATE_SESSION)?,
    )?;
    Ok(Established { clientid, id, fore })
}

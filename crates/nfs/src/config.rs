use crate::transport::{Security, Transport};
use nfs_rpc::Auth;
use std::time::Duration;

/// How to reach one server and whom to be there.
#[derive(Clone)]
pub struct Config {
    pub transport: Transport,
    pub security: Security,
    /// AUTH_SYS: the identity export rules (squashing, permissions) apply to.
    pub auth: Auth,
    /// Names this client for good (one per installation): a new run of the same client makes the
    /// server drop what the previous one left behind.
    pub owner: String,
    /// Connections (TCP) or CONNECT streams (QUIC) in the session. By default 4 streams for QUIC
    /// (stage 1), and 8 connections for TCP (servers without a gateway): separate lanes for what a
    /// reader waits for, without taking many of the server's connections (nfsd drops those past
    /// (threads + 3) × 20).
    pub connections: usize,
    /// A connection with calls waiting and nothing arriving for this long is dropped.
    pub idle_timeout: Duration,
    /// How long the server has to answer at all when a connection is made (name lookup and TCP
    /// connect, or the tunnel's QUIC handshake and CONNECT): with nobody there, fail now rather
    /// than after the idle timeout. What follows (TLS, the session) may be slow but is under way.
    pub reach_timeout: Duration,
    /// How long a call keeps being retried, across reconnections, before it fails.
    pub call_timeout: Duration,
}

impl Config {
    pub fn new(
        transport: Transport,
        security: Security,
        auth: Auth,
        owner: impl Into<String>,
    ) -> Self {
        let owner = owner.into();
        let connections = match transport {
            Transport::Quic(_) => 4,
            Transport::Tcp(_) => 8,
        };
        Self {
            transport,
            security,
            auth,
            owner,
            connections,
            idle_timeout: Duration::from_secs(30),
            reach_timeout: Duration::from_secs(4),
            call_timeout: Duration::from_secs(75),
        }
    }
}

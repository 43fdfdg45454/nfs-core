//! What a client's connections are doing now, for a live view (diagnostics, a notification).

/// A snapshot of the session's connections.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// QUIC streams through the gateway, or else TCP connections to nfsd.
    pub quic: bool,
    /// Connections (or streams) the session keeps, and how many are up now.
    pub lanes: usize,
    pub alive: usize,
    /// Calls waiting for their reply.
    pub in_flight: usize,
    /// Connections made since the start: more than `lanes` means reconnections.
    pub connects: u32,
    /// The QUIC path to the gateway, while connected.
    pub path: Option<nfs_tunnel::client::Path>,
}

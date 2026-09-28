//! What the session tells about itself.

use super::Session;
use std::sync::atomic::Ordering;

impl Session {
    pub async fn clientid(&self) -> u64 {
        self.state.read().await.clientid
    }

    /// Largest READ or WRITE payload the session carries.
    pub async fn max_io(&self) -> u32 {
        let fore = self.state.read().await.fore;
        (fore.max_request.min(fore.max_response) - (4 << 10)).min(1 << 20)
    }

    pub(super) fn touch(&self) {
        self.last_call.store(self.origin.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    pub fn owner(&self) -> &str {
        &self.config.owner
    }

    /// How many times the server said it could not call the client back (a diagnostic).
    pub fn callbacks_down(&self) -> u32 {
        self.channels.rebinds.load(Ordering::Relaxed)
    }

    /// CB_NULL and CB_COMPOUND calls received, and SEQUENCE's last status flags (diagnosis).
    pub fn callback_counts(&self) -> [u32; 3] {
        self.callbacks.seen.each_ref().map(|c| c.load(Ordering::Relaxed))
    }

    /// The connections now: up, busy, remade; over QUIC, the path to the gateway.
    pub fn stats(&self) -> crate::Stats {
        let (alive, in_flight) = self.channels.load();
        let tunnel = match &self.config.transport {
            crate::Transport::Quic(tunnel) => Some(tunnel),
            crate::Transport::Tcp(_) => None,
        };
        crate::Stats {
            quic: tunnel.is_some(),
            lanes: self.channels.connections.len(),
            alive,
            in_flight,
            connects: self.channels.connects.load(Ordering::Relaxed),
            path: tunnel.and_then(|t| t.path()),
        }
    }

    pub fn connections(&self) -> usize {
        self.config.connections
    }

    pub async fn network_changed(&self) {
        self.channels.restart().await;
    }

    pub fn close(&self) {
        self.channels.close();
    }
}

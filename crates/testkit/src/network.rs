//! Breaking the client's network on purpose, where the test may (the CI's client namespace).

use super::{config, env, export};
use nfs_client::Client;
use std::sync::Arc;

/// Whether this run may break the client's network (the CI's client namespace, as root).
pub fn can_break() -> bool {
    env("NFS_CAN_BREAK").is_some()
}

/// Runs a command that changes the network; the test fails if it cannot.
pub fn run(command: &str) {
    let status = std::process::Command::new("sh").args(["-c", command]).status().expect("sh");
    assert!(status.success(), "{command}");
}

/// Replies from the server (nfsd or the gateway) are dropped until the guard goes.
pub struct Deaf;

impl Deaf {
    #[allow(clippy::new_without_default)] // Making it has an effect: no Default.
    pub fn new() -> Self {
        run(
            "nft add table inet deaf && nft add chain inet deaf input '{ type filter hook input priority 0; }' \
             && nft add rule inet deaf input ip saddr 198.51.100.1 drop",
        );
        Self
    }
}

impl Drop for Deaf {
    fn drop(&mut self) {
        run("nft delete table inet deaf");
    }
}

/// A client whose connections give up after `idle` seconds without replies.
pub async fn client_with_idle(idle: u64) -> Option<Arc<Client>> {
    let mut config = config()?;
    config.idle_timeout = std::time::Duration::from_secs(idle);
    Some(Client::connect(config, &export()).await.expect("connect"))
}

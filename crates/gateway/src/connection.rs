//! One QUIC connection: every request on it becomes a tunnel from the connection's address.

use crate::{Target, tunnel};
use std::sync::Arc;

pub async fn serve(incoming: quinn::Incoming, target: Arc<Target>) {
    let peer = incoming.remote_address();
    let Some(target) = target.reached_at(incoming.local_ip()).map(Arc::new) else {
        return eprintln!("{peer}: the address it reached is unknown, so is nfsd's");
    };
    let result = async {
        let connection = incoming.await?;
        let mut h3 = h3::server::Connection::new(h3_quinn::Connection::new(connection)).await?;
        while let Some(resolver) = h3.accept().await? {
            // The source is the address the packets came from, and nothing else: migration is
            // off, so it cannot change for the life of the connection.
            tokio::spawn(tunnel::serve(resolver, peer.ip().to_canonical(), target.clone()));
        }
        Ok::<_, nfs_tunnel::Error>(())
    };
    if let Err(error) = result.await {
        eprintln!("{peer}: {error}");
    }
}

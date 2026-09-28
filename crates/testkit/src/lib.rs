//! What every test against a real server shares. The server comes from the environment:
//! NFS_SERVER (host:port of nfsd), NFS_EXPORT, NFS_TLS (tls or mtls, with NFS_TLS_DIR holding
//! ca.pem and client.pem/client.key), NFS_GATEWAY (host:port: QUIC through the gateway, with
//! NFS_GATEWAY_NAME). Without NFS_SERVER the tests do nothing.

use nfs_client::{Client, Config, Fh, Security, Transport};
use nfs_rpc::{Auth, SysCred};
use nfs_tunnel::client::Tunnel;
use std::path::PathBuf;
use std::sync::Arc;

pub fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

pub fn tls_dir() -> PathBuf {
    PathBuf::from(env("NFS_TLS_DIR").unwrap_or_default())
}

/// RPC-with-TLS trusting the test CA, as `name`, with the client identity `identity` (a file
/// name prefix in NFS_TLS_DIR, such as "client"), if any.
pub fn tls(name: &str, identity: Option<&str>) -> Security {
    let dir = tls_dir();
    let files = identity.map(|id| (dir.join(format!("{id}.pem")), dir.join(format!("{id}.key"))));
    let tls = nfs_tunnel::tls::client(
        &dir.join("ca.pem"),
        files.as_ref().map(|(c, k)| (c.as_path(), k.as_path())),
    );
    Security::tls(tls.expect("TLS configuration"), name).expect("server name")
}

pub fn host() -> Option<String> {
    let server = env("NFS_SERVER")?;
    Some(server.rsplit_once(':').map_or(server.as_str(), |(h, _)| h).to_owned())
}

pub fn config() -> Option<Config> {
    let server = env("NFS_SERVER")?;
    let dir = tls_dir();
    let identity = (dir.join("client.pem"), dir.join("client.key"));
    let host = host()?;
    let security = match env("NFS_TLS").as_deref() {
        None => Security::None,
        Some(mode) => tls(&host, (mode == "mtls").then_some("client")),
    };
    let transport = match env("NFS_GATEWAY") {
        None => Transport::Tcp(server.clone()),
        Some(gateway) => {
            let tls =
                nfs_tunnel::tls::client(&dir.join("ca.pem"), Some((&identity.0, &identity.1)))
                    .expect("TLS");
            let endpoint =
                nfs_tunnel::quic::client(tls, Default::default()).expect("QUIC endpoint");
            let server_name = env("NFS_GATEWAY_NAME").unwrap_or(host);
            let authority = server.clone();
            let gateway = gateway.parse().expect("gateway address");
            Transport::Quic(Arc::new(Tunnel {
                endpoint,
                gateway,
                server_name,
                authority,
                header: None,
                send_request: Default::default(),
            }))
        }
    };
    let auth = Auth::Sys(SysCred { machine: "nfs-core-test".into(), uid: 0, gid: 0, gids: vec![] });
    // One owner per client: two clients with the same owner would each look like the other
    // restarting, and the server would drop the other's session.
    static CLIENTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = CLIENTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Some(Config::new(
        transport,
        security,
        auth,
        format!("nfs-core-test-{}-{n}", std::process::id()),
    ))
}

pub fn export() -> String {
    env("NFS_EXPORT").unwrap_or_else(|| "/".into())
}

/// A client and a fresh directory for one test, or `None` without a server.
pub async fn setup(test: &str) -> Option<(Arc<Client>, Fh)> {
    let client = Client::connect(config()?, &export()).await.expect("connect");
    let name = format!("{test}-{}-{:?}", std::process::id(), std::time::SystemTime::now());
    let (dir, _) = client
        .mkdir(client.root(), &name.replace([' ', ':', '.'], "-"), &Default::default())
        .await
        .unwrap_or_else(|e| panic!("test directory: {e}"));
    Some((client, dir))
}

mod network;
mod nfsd;
pub use network::*;
pub use nfsd::*;

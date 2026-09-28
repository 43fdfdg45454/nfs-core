//! nfs-gateway: accepts QUIC connections and carries each HTTP/3 `CONNECT` stream to nfsd as a
//! TCP connection opened from the client's own address. It never reads the RPC or the export's
//! TLS, and has no permissions of its own: nfsd and /etc/exports decide as without it.
//! Every option can also come from the environment: `--client-ca` as `NFS_GATEWAY_CLIENT_CA`.

mod connection;
mod source;
mod tunnel;

use nfs_tunnel::args::Args;
use nfs_tunnel::congestion::Congestion;
use nfs_tunnel::quic;
use nfs_tunnel::{Error, tls};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

/// Where tunnels go, and the socket mark that routes nfsd's replies back to the gateway.
pub struct Target {
    pub addr: SocketAddr,
    pub mark: u32,
}

impl Target {
    /// For a client that reached this host at `local`: an unspecified or loopback address (the
    /// default is 0.0.0.0:2049) means this host, at that address. Loopback itself would not do:
    /// the kernel drops packets to it from the client's (outside) address.
    pub fn reached_at(&self, local: Option<IpAddr>) -> Option<Target> {
        let ip = self.addr.ip();
        let ip = if ip.is_unspecified() || ip.is_loopback() { local?.to_canonical() } else { ip };
        Some(Target { addr: SocketAddr::new(ip, self.addr.port()), mark: self.mark })
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse(
        &["listen", "target", "cert", "key", "client-ca", "no-client-auth", "congestion", "mark"],
        Some("NFS_GATEWAY"),
    )?;
    // A client certificate is asked for unless explicitly turned off (which wins over a CA the
    // image sets by default).
    let no_client_auth = args.flag("no-client-auth");
    let client_ca: Option<PathBuf> = args.get("client-ca")?.filter(|_| !no_client_auth);
    if client_ca.is_none() && !no_client_auth {
        return Err("--client-ca is required unless --no-client-auth is given".into());
    }
    let cert: PathBuf = args.required("cert")?;
    let key: PathBuf = args.required("key")?;
    let tls = tls::server(&cert, &key, client_ca.as_deref())?;
    let congestion: Congestion = args.get("congestion")?.unwrap_or_default();
    let listen: SocketAddr = args.required("listen")?;
    let endpoint = quic::server(listen, tls, congestion)?;
    let addr = args.get("target")?.unwrap_or(SocketAddr::from(([0, 0, 0, 0], 2049)));
    let target = Arc::new(Target { addr, mark: mark(args.get("mark")?)? });
    eprintln!(
        "listening on {listen}, tunnels to {addr} (0.0.0.0 or 127.0.0.1: this host), {congestion:?}"
    );
    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(connection::serve(incoming, target.clone()));
    }
    Ok(())
}

/// The socket mark, in decimal or in hexadecimal (0x...), as nft and ip rule take it.
fn mark(value: Option<String>) -> Result<u32, Error> {
    let Some(value) = value else { return Ok(0x4e46) };
    let parsed = match value.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => value.parse(),
    };
    parsed.map_err(|e| format!("--mark {value}: {e}").into())
}

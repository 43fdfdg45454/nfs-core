//! nfs-tunnel-client: every TCP connection accepted on --listen travels to the gateway as an
//! HTTP/3 CONNECT stream, all of them over one QUIC connection. It stands in for the client
//! core's QUIC transport, so that existing NFS clients (the kernel's) can be measured over
//! it before that core exists.

use nfs_tunnel::args::Args;
use nfs_tunnel::client::Tunnel;
use nfs_tunnel::congestion::Congestion;
use nfs_tunnel::quic;
use nfs_tunnel::{Error, tls};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse(
        &[
            "listen",
            "gateway",
            "server-name",
            "authority",
            "ca",
            "cert",
            "key",
            "congestion",
            "header",
        ],
        None,
    )?;
    let (cert, key): (Option<PathBuf>, Option<PathBuf>) = (args.get("cert")?, args.get("key")?);
    let tls = tls::client(&args.required::<PathBuf>("ca")?, cert.as_deref().zip(key.as_deref()))?;
    let congestion: Congestion = args.get("congestion")?.unwrap_or_default();
    let server_name: String = args.required("server-name")?;
    let session = Arc::new(Tunnel {
        endpoint: quic::client(tls, congestion)?,
        gateway: args.required("gateway")?,
        authority: args.get("authority")?.unwrap_or(format!("{server_name}:2049")),
        server_name,
        header: args.get("header")?,
        send_request: Default::default(),
    });
    let listen: SocketAddr = args.required("listen")?;
    let listener = TcpListener::bind(listen).await?;
    eprintln!("listening on {listen}, tunnels to {} ({congestion:?})", session.gateway);
    loop {
        let (tcp, _) = listener.accept().await?;
        let session = session.clone();
        tokio::spawn(async move {
            _ = tcp.set_nodelay(true);
            match session.carry(tcp).await {
                Ok((up, down)) => eprintln!("tunnel: {up} bytes up, {down} down"),
                Err(error) => eprintln!("tunnel: {error}"),
            }
        });
    }
}

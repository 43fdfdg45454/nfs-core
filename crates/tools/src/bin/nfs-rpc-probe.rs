//! nfs-rpc-probe: how long a small RPC waits while the link is busy. Sends an NFSv4 NULL call
//! (RFC 5531, no session needed) every 100 ms on its own TCP connection and reports the round
//! trips. Through the tunnel client, that connection is its own CONNECT stream.

use nfs_tunnel::Error;
use nfs_tunnel::args::Args;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Record mark (last fragment, 40 bytes), then xid, CALL, RPC 2, NFS (100003) v4, NULL, and
/// AUTH_NONE credential and verifier.
fn null_call(xid: u32) -> Vec<u8> {
    [0x8000_0028, xid, 0, 2, 100_003, 4, 0, 0, 0, 0, 0]
        .iter()
        .flat_map(|w: &u32| w.to_be_bytes())
        .collect()
}

async fn round_trip(tcp: &mut TcpStream, xid: u32) -> Result<Duration, Error> {
    let start = Instant::now();
    tcp.write_all(&null_call(xid)).await?;
    let mark = tcp.read_u32().await?;
    let mut reply = vec![0; (mark & 0x7fff_ffff) as usize];
    tcp.read_exact(&mut reply).await?;
    if mark & 0x8000_0000 == 0 || reply.get(..4) != Some(&xid.to_be_bytes()[..]) {
        return Err("unexpected reply".into());
    }
    Ok(start.elapsed())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse(&["server", "seconds", "label"], None)?;
    let server: SocketAddr = args.required("server")?;
    let seconds: u64 = args.get("seconds")?.unwrap_or(15);
    let label: String = args.get("label")?.unwrap_or_default();
    let mut tcp = TcpStream::connect(server).await?;
    tcp.set_nodelay(true)?;
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut samples = Vec::new();
    for xid in 1.. {
        samples.push(round_trip(&mut tcp, xid).await?);
        if Instant::now() >= end {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    samples.sort();
    let p = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize].as_secs_f64() * 1e3;
    println!(
        "{label}NULL RPC round trip: n={} min {:.0} p50 {:.0} p95 {:.0} max {:.0} ms",
        samples.len(),
        p(0.0),
        p(0.5),
        p(0.95),
        p(1.0)
    );
    Ok(())
}

//! Sustained reading or writing through nfs-core's NFSv4.2 client, to measure a transport (stage 1)
//! and to check the export's TLS through the tunnel. The server comes from the environment as in
//! the tests (nfs-testkit: NFS_SERVER, NFS_EXPORT, NFS_TLS...), plus NFS_CONNECTIONS (lanes),
//! NFS_IN_FLIGHT (1 MiB calls in flight per lane, 8), NFS_SECONDS (20), NFS_FILE (read,
//! "bench.bin") and NFS_WRITE (write a new file instead). Prints "read|write <MB/s> MB/s".

use nfs_client::{Create, READ_ACCESS, WRITE_ACCESS};
use nfs_testkit as kit;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

const BLOCK: u64 = 1 << 20;

fn number(name: &str, default: u64) -> u64 {
    kit::env(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = kit::config().ok_or("NFS_SERVER is not set")?;
    config.connections = number("NFS_CONNECTIONS", config.connections as u64) as usize;
    let calls = config.connections * number("NFS_IN_FLIGHT", 8) as usize;
    let seconds = Duration::from_secs(number("NFS_SECONDS", 20));
    let client = nfs_client::Client::connect(config, &kit::export()).await?;
    let write = kit::env("NFS_WRITE").is_some();
    let (file, size, name) = if write {
        let name = format!("bench-{}", std::process::id());
        let create = Create::Guarded(Default::default());
        (client.create(client.root(), &name, create, WRITE_ACCESS).await?.0, u64::MAX, Some(name))
    } else {
        let (fh, attrs) =
            client.lookup(None, &kit::env("NFS_FILE").unwrap_or("bench.bin".into())).await?;
        (client.open(&fh, READ_ACCESS).await?, attrs.size, None)
    };
    let (file, next, moved) =
        (Arc::new(file), Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let data = Arc::new(vec![0x5a_u8; BLOCK as usize]);
    let end = Instant::now() + seconds;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..calls {
        let (file, next, moved, data) = (file.clone(), next.clone(), moved.clone(), data.clone());
        tasks.spawn(async move {
            while Instant::now() < end {
                // Blocks in order, back to the start at the end of the file.
                let offset = (next.fetch_add(BLOCK, Relaxed) % size.max(BLOCK)) / BLOCK * BLOCK;
                let n = if write {
                    u64::from(file.write(offset, &data).await?.0)
                } else {
                    file.read(offset, BLOCK as u32, true).await?.0.len() as u64
                };
                moved.fetch_add(n, Relaxed);
            }
            Ok::<_, nfs_client::Error>(())
        });
    }
    while let Some(result) = tasks.join_next().await {
        result??;
    }
    let mbs = moved.load(Relaxed) as f64 / seconds.as_secs_f64() / 1e6;
    println!("{} {mbs:.1} MB/s", if write { "write" } else { "read" });
    file.close().await?;
    if let Some(name) = name {
        client.remove(client.root(), &name).await?;
    }
    Ok(())
}

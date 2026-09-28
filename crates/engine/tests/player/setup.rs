//! Getting an engine on the test server, the fixtures, and reporting against the limits.

use nfs_engine::{Engine, Reader};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn check(label: u64, offset: u64, data: &[u8]) {
    for (i, word) in data.as_chunks::<8>().0.iter().enumerate() {
        let expected = (offset / 8 + i as u64) | (label << 48);
        let got = u64::from_le_bytes(*word);
        assert_eq!(got, expected, "file {label} at {}", offset + 8 * i as u64);
    }
}

/// A fixture by name, with its label, from the export's `fixtures` directory.
pub async fn open(engine: &Arc<Engine>, name: &str) -> (Reader, u64) {
    let (fh, _) = engine.client().lookup(None, &format!("fixtures/{name}")).await.expect("fixture");
    let label = u64::from(name.as_bytes()[name.len() - 5]);
    (engine.read(&fh).await.expect("open"), label)
}

/// An engine on the test server, with a disk cache of its own, or `None` without a server.
pub async fn engine() -> Option<Arc<Engine>> {
    engine_with(true).await
}

pub async fn engine_with(disk_cache: bool) -> Option<Arc<Engine>> {
    let config = nfs_testkit::config()?;
    let client =
        nfs_client::Client::connect(config, &nfs_testkit::export()).await.expect("connect");
    let dir = std::env::temp_dir().join(format!(
        "nfs-engine-test-{}-{:?}",
        std::process::id(),
        Instant::now()
    ));
    let cache = disk_cache.then(|| nfs_engine::DiskCache::new(dir, 4 << 30).expect("cache"));
    Some(Engine::new(client, nfs_engine::Config { cache, ..Default::default() }))
}

/// A line for the CI's annotation.
pub fn report(name: &str, line: impl std::fmt::Display) {
    println!("RESULT {name}: {line}");
}

/// Fails on a good-experience limit (nfs-limits) where they are enforced (NFS_ENFORCE_LIMITS).
pub fn limit(what: &str, samples: &[Duration], mean: Option<Duration>, max: Duration) {
    match nfs_limits::check(what, samples, mean, max) {
        Ok(summary) => report(what, summary),
        Err(why) if std::env::var("NFS_ENFORCE_LIMITS").is_ok() => panic!("{why}"),
        Err(why) => report(what, format!("over the limit (not enforced): {why}")),
    }
}

/// After the last reader closed: the engine is idle, and nfsd holds no open state of its client,
/// within 5 s (a READ of 1 MiB over the VPN link may still be finishing).
pub async fn no_leaks(engine: &Arc<Engine>) {
    let start = Instant::now();
    loop {
        let opens = nfs_testkit::server_opens(engine.client().owner()).unwrap_or(0);
        if engine.idle() && opens == 0 {
            return report("no leaks", format!("idle after {:?}", start.elapsed()));
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "leak: engine idle {}, {opens} opens on the server",
            engine.idle()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Reads one chunk at `offset` and checks where it came from.
pub async fn source(reader: &Reader, offset: u64) -> nfs_engine::Source {
    reader.read_at(offset, super::CHUNK as usize).await.expect("read");
    reader.last_source()
}

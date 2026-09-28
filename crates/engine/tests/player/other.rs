//! Another client (another session, as another device would be) changing the fixtures, and what
//! the app sees of it.

use super::{CHUNK, check};
use nfs_client::attr::SetAttrs;
use nfs_client::{Client, Create, Fh, WRITE_ACCESS};
use nfs_engine::{Engine, Reader, Source};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub async fn other() -> Arc<Client> {
    let config = nfs_testkit::config().expect("server");
    Client::connect(config, &nfs_testkit::export()).await.expect("connect")
}

async fn fixtures(client: &Client) -> Fh {
    client.lookup(None, "fixtures").await.expect("fixtures").0
}

/// Writes (creating it if missing) `len` bytes of the fixtures' pattern with `label` at `offset`.
pub async fn write(name: &str, offset: u64, len: u64, label: u64) {
    let other = other().await;
    let dir = fixtures(&other).await;
    let create = Create::Unchecked(Default::default());
    let (file, _) = other.create(&dir, name, create, WRITE_ACCESS).await.expect("open");
    for at in (offset..offset + len).step_by(1 << 20) {
        let words = at / 8..(at + (1 << 20)).min(offset + len) / 8;
        let data: Vec<u8> = words.flat_map(|w| (w | label << 48).to_le_bytes()).collect();
        assert_eq!(file.write(at, &data).await.expect("write").0 as usize, data.len());
    }
    file.commit().await.expect("commit");
    file.close().await.expect("close");
    other.close();
}

pub async fn truncate(name: &str, size: u64) {
    let other = other().await;
    let (fh, _) = other.lookup(Some(&fixtures(&other).await), name).await.expect("fixture");
    other.setattr(&fh, &SetAttrs { size: Some(size), ..Default::default() }).await.expect("size");
    other.close();
}

pub async fn remove(name: &str) {
    let other = other().await;
    other.remove(&fixtures(&other).await, name).await.expect("remove");
    other.close();
}

/// Opening `name` again fails as missing; how long it took to know.
pub async fn open_fails(engine: &Arc<Engine>, name: &str) -> Duration {
    let start = Instant::now();
    let found = engine.client().lookup(None, &format!("fixtures/{name}")).await;
    let error = found.err().unwrap_or_else(|| panic!("{name} opened after it was deleted"));
    assert_eq!(std::io::Error::from(error).kind(), std::io::ErrorKind::NotFound);
    start.elapsed()
}

/// A jump: one chunk at `offset`, checked against `label`; how long it took and where it came from.
pub async fn seek(reader: &Reader, label: u64, offset: u64) -> (Duration, Source) {
    let start = Instant::now();
    let data = reader.read_at(offset, CHUNK as usize).await.expect("read");
    assert_eq!(data.len() as u64, CHUNK.min(reader.size() - offset), "bytes at {offset}");
    check(label, offset, &data);
    (start.elapsed(), reader.last_source())
}

/// Waits up to 5 s for the disk cache to hold at most `versions` of the file.
pub async fn cached_at_most(engine: &Engine, fh: &Fh, versions: usize) {
    let cache = engine.cache().expect("disk cache");
    let start = Instant::now();
    while cache.versions(fh) > versions {
        assert!(start.elapsed() < Duration::from_secs(5), "{} versions cached", cache.versions(fh));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

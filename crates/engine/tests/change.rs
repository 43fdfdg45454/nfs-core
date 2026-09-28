//! Files changed by another client while played, with close-to-open consistency like the
//! kernel's client: while open, what was read may stay old; once reopened, everything is the new
//! version, from the network, and only it stays cached. A file that grows can be read past its old
//! end through the same reader; one that shrinks ends at its new end at once, without hanging.

mod player;

use nfs_engine::Source;
use nfs_limits::{OPEN, SEEK_MAX, SEEK_MEAN};
use player::{CHUNK, check, limit, report};
use std::time::{Duration, Instant};

const MIB: u64 = 1 << 20;

#[tokio::test(flavor = "multi_thread")]
async fn modified_by_another_client_while_playing() {
    let Some(engine) = player::engine().await else { return };
    let name = "edited-i.bin";
    let (reader, label) = player::open(&engine, name).await;
    let (size, new) = (reader.size(), label ^ 0x20);
    let at = |tenth: u64| tenth * size / 10 / MIB * MIB;
    let mut stalled = vec![player::play(&reader, label, 0, 5).await.stalled];
    stalled.push(player::play(&reader, label, at(5), 3).await.stalled);
    player::write(name, 0, 4 * MIB, new).await;
    player::write(name, at(8), 4 * MIB, new).await;
    // A part neither read nor changed plays on.
    stalled.push(player::play(&reader, label, at(3), 3).await.stalled);
    drop(reader);
    let (reader, _) = player::open(&engine, name).await;
    for (offset, label) in [(0, new), (at(8), new), (at(5), label)] {
        let (_, source) = player::seek(&reader, label, offset).await;
        assert_eq!(source, Source::Network, "the new version at {offset} came from the old one");
    }
    let fh = engine.client().lookup(None, &format!("fixtures/{name}")).await.unwrap().0;
    drop(reader);
    player::cached_at_most(&engine, &fh, 1).await;
    limit("modified: stalled", &stalled, None, Duration::ZERO);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn appended_by_another_client_while_playing() {
    let Some(engine) = player::engine().await else { return };
    let name = "grown-j.bin";
    let (reader, label) = player::open(&engine, name).await;
    let size = reader.size();
    let near = reader.read_at(size - 5 * MIB, 3 * MIB as usize).await.unwrap();
    check(label, size - 5 * MIB, &near);
    player::write(name, size, 8 * MIB, label).await;
    let mut reads = Vec::new();
    for offset in (size..size + 8 * MIB).step_by(MIB as usize) {
        let start = Instant::now();
        let data = reader.read_at(offset, MIB as usize).await.unwrap();
        reads.push(start.elapsed());
        assert_eq!(data.len() as u64, MIB, "appended part at {offset}");
        check(label, offset, &data);
    }
    drop(reader);
    let (reader, _) = player::open(&engine, name).await;
    assert_eq!(reader.size(), size + 8 * MIB, "size after growing");
    player::seek(&reader, label, reader.size() - CHUNK).await;
    drop(reader);
    limit("appended: 1 MiB reads", &reads, Some(SEEK_MEAN), SEEK_MAX);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn truncated_by_another_client_while_playing() {
    let Some(engine) = player::engine().await else { return };
    let name = "shrunk-k.bin";
    let (reader, label) = player::open(&engine, name).await;
    let (size, new_size) = (reader.size(), reader.size() / 4 / MIB * MIB);
    check(label, 0, &reader.read_at(0, MIB as usize).await.unwrap());
    player::truncate(name, new_size).await;
    let start = Instant::now();
    let past = reader.read_at(size - MIB, MIB as usize).await.unwrap();
    let past_end = start.elapsed();
    assert!(past.is_empty(), "{} bytes read past the new end", past.len());
    let (before, _) = player::seek(&reader, label, new_size / 2).await;
    drop(reader);
    let (reader, _) = player::open(&engine, name).await;
    assert_eq!(reader.size(), new_size, "size after truncating");
    player::seek(&reader, label, new_size - CHUNK).await;
    drop(reader);
    limit("truncated: read past the new end", &[past_end], None, OPEN);
    limit("truncated: read before the new end", &[before], Some(SEEK_MEAN), SEEK_MAX);
    player::no_leaks(&engine).await;
}

/// A file opened again while another reader has it, with the read delegation still held: nobody
/// changed it, so nothing is asked of the server (over the VPN, well under one round trip).
#[tokio::test(flavor = "multi_thread")]
async fn reopening_a_delegated_file_asks_nothing() {
    let Some(engine) = player::engine().await else { return };
    let fh = engine.client().lookup(None, "fixtures/movie-c.bin").await.unwrap().0;
    // nfsd checks the callback path after the session starts: a delegation may take a moment.
    let start = Instant::now();
    let (first, label) = loop {
        let (first, label) = player::open(&engine, "movie-c.bin").await;
        player::seek(&first, label, 0).await;
        if engine.client().delegation(&fh).is_some() || start.elapsed() > Duration::from_secs(5) {
            break (first, label);
        }
        drop(first);
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    if engine.client().delegation(&fh).is_none() {
        assert!(nfs_testkit::env("NFS_REQUIRE_DELEGATIONS").is_none(), "no delegation granted");
        return report("reopen with a delegation", "no delegation granted");
    }
    let start = Instant::now();
    let second = engine.read(&fh).await.unwrap();
    let took = start.elapsed();
    assert_eq!(player::seek(&second, label, 0).await.1, Source::Memory);
    limit("reopen with a delegation", &[took], None, Duration::from_millis(20));
    drop((first, second));
    player::no_leaks(&engine).await;
}

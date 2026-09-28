//! Files deleted while played, as a local file on Linux: playback and jumps go on to the end (the
//! server keeps an open file's data until it is closed); opening it again fails at once. Deleted
//! through the engine, its cache is dropped; by another client, the engine cannot know, but that
//! data is never served for a new file at the same name. And with the disk cache off, nothing
//! read before is kept once the file is closed.

mod player;

use nfs_engine::Source;
use nfs_limits::{OPEN, SEEK_MAX, SEEK_MEAN};
use player::{CHUNK, limit};
use std::time::{Duration, Instant};

const MIB: u64 = 1 << 20;

#[tokio::test(flavor = "multi_thread")]
async fn deleted_in_the_app_while_playing() {
    let Some(engine) = player::engine().await else { return };
    let name = "gone-g.bin";
    let (reader, label) = player::open(&engine, name).await;
    let at = |tenth: u64| tenth * reader.size() / 10 / MIB * MIB;
    let fh = engine.client().lookup(None, &format!("fixtures/{name}")).await.unwrap().0;
    let mut stalled = vec![player::play(&reader, label, at(1), 5).await.stalled];
    let dir = engine.client().lookup(None, "fixtures").await.unwrap().0;
    let start = Instant::now();
    engine.remove(&dir, name).await.unwrap();
    let removed = start.elapsed();
    stalled.push(player::play(&reader, label, at(1) + 5 * MIB, 5).await.stalled);
    let played = player::play(&reader, label, at(7), 3).await;
    stalled.push(played.stalled);
    drop(reader);
    let reopen = player::open_fails(&engine, name).await;
    player::cached_at_most(&engine, &fh, 0).await;
    limit("deleted in the app: delete", &[removed], None, OPEN);
    limit("deleted in the app: stalled", &stalled, None, Duration::ZERO);
    limit("deleted in the app: seek", &[played.first_byte], Some(SEEK_MEAN), SEEK_MAX);
    limit("deleted in the app: reopening fails", &[reopen], None, OPEN);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deleted_by_another_client_then_replaced() {
    let Some(engine) = player::engine().await else { return };
    let name = "gone-h.bin";
    let (reader, label) = player::open(&engine, name).await;
    let at = |tenth: u64| tenth * reader.size() / 10 / MIB * MIB;
    let mut stalled = vec![player::play(&reader, label, 0, 5).await.stalled];
    stalled.push(player::play(&reader, label, at(5), 3).await.stalled);
    player::remove(name).await;
    let (seen, _) = player::seek(&reader, label, 2 * MIB).await;
    let played = player::play(&reader, label, at(7), 3).await;
    stalled.push(played.stalled);
    drop(reader);
    let reopen = player::open_fails(&engine, name).await;
    // A different file at the same name: all of it from the network.
    let new = label ^ 0x20;
    player::write(name, 0, 8 * MIB, new).await;
    let (reader, _) = player::open(&engine, name).await;
    for offset in [0, 2 * MIB, 8 * MIB - CHUNK] {
        let (_, source) = player::seek(&reader, new, offset).await;
        assert_eq!(source, Source::Network, "the new file at {offset} came from the old one");
    }
    drop(reader);
    limit("deleted by another client: stalled", &stalled, None, Duration::ZERO);
    let seeks = [seen, played.first_byte];
    limit("deleted by another client: seek", &seeks, Some(SEEK_MEAN), SEEK_MAX);
    limit("deleted by another client: reopening fails", &[reopen], None, OPEN);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_disk_cache_off_nothing_is_kept_after_closing() {
    let Some(engine) = player::engine_with(false).await else { return };
    let (reader, label) = player::open(&engine, "movie-a.bin").await;
    let places = [5, 30, 60].map(|percent| percent * reader.size() / 100 / CHUNK * CHUNK);
    for &place in &places {
        player::seek(&reader, label, place).await;
    }
    drop(reader);
    player::no_leaks(&engine).await;
    let (reader, _) = player::open(&engine, "movie-a.bin").await;
    for &place in &places {
        let (_, source) = player::seek(&reader, label, place).await;
        assert_eq!(source, Source::Network, "{place} was kept with the disk cache off");
    }
    drop(reader);
    player::no_leaks(&engine).await;
}

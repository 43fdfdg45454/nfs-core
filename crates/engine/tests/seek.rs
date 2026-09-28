//! Looking for a scene: jumps with a few seconds played after each; the same places again must
//! come from the cache, and a place never read must come from the network. The scenes are in the
//! first 60 % of the file, so that read-ahead never reaches its end.

mod player;

use nfs_engine::Source;
use nfs_limits::{SEEK_MAX, SEEK_MEAN, SEEN};
use player::{CHUNK, limit};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn scenes_and_the_same_scenes_again() {
    let Some(engine) = player::engine().await else { return };
    let (reader, label) = player::open(&engine, "movie-c.bin").await;
    let places: Vec<u64> =
        (0..12u64).map(|i| (i * 7919 % 97) * (reader.size() * 6 / 1000) / CHUNK * CHUNK).collect();
    let (mut seeks, mut stalled) = (Vec::new(), Vec::new());
    for &place in &places {
        let played = player::play(&reader, label, place, 3).await;
        seeks.push(played.first_byte);
        stalled.push(played.stalled);
    }
    limit("seek", &seeks, Some(SEEK_MEAN), SEEK_MAX);
    limit("scenes stalled", &stalled, None, Duration::ZERO);
    drop(reader);
    let (reader, _) = player::open(&engine, "movie-c.bin").await;
    let mut seen = Vec::new();
    for &place in &places {
        let start = Instant::now();
        assert_ne!(
            player::source(&reader, place).await,
            Source::Network,
            "{place} was read before"
        );
        seen.push(start.elapsed());
    }
    limit("already seen", &seen, None, SEEN);
    let never = reader.size() - 3 * CHUNK;
    assert_eq!(player::source(&reader, never).await, Source::Network, "{never} was never read");
    drop(reader);
    player::no_leaks(&engine).await;
}

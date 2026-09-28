//! Scrubbing: a burst of jumps (a chunk read at each, as a player shows a frame) and then playing
//! where it settles; and a jump right after opening.

mod player;

use nfs_limits::{OPEN, SEEK_MAX, SEEK_MEAN};
use player::{CHUNK, limit};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn thirty_jumps_then_play() {
    let Some(engine) = player::engine().await else { return };
    let (reader, label) = player::open(&engine, "movie-a.bin").await;
    let mut jumps = Vec::new();
    for i in 0..30u64 {
        let place = (i * 104_729 % 991) * (reader.size() / 1000) / CHUNK * CHUNK;
        let start = Instant::now();
        let data = reader.read_at(place, CHUNK as usize).await.unwrap();
        player::check(label, place, &data);
        jumps.push(start.elapsed());
    }
    limit("burst jump", &jumps, Some(SEEK_MEAN), SEEK_MAX);
    let settled = player::play(&reader, label, reader.size() / 2 / CHUNK * CHUNK, 10).await;
    limit("after the burst: seek", &[settled.first_byte], None, SEEK_MAX);
    limit("after the burst: stalled", &[settled.stalled], None, Duration::ZERO);
    drop(reader);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_jump_right_after_opening() {
    let Some(engine) = player::engine().await else { return };
    let start = Instant::now();
    let (reader, label) = player::open(&engine, "movie-b.bin").await;
    let opened = start.elapsed();
    let played = player::play(&reader, label, reader.size() * 7 / 10 / CHUNK * CHUNK, 5).await;
    limit("open and jump", &[opened + played.first_byte], None, OPEN);
    limit("after the jump: stalled", &[played.stalled], None, Duration::ZERO);
    drop(reader);
    player::no_leaks(&engine).await;
}

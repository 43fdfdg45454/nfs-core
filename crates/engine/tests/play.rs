//! Playing: a whole movie, an episode marathon, and a file read as fast as the link allows.

mod player;

use nfs_engine::Source;
use nfs_limits::OPEN;
use player::{CHUNK, check, limit, report};
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread")]
async fn a_movie_and_an_episode_marathon() {
    let Some(engine) = player::engine().await else { return };
    let (mut opens, mut stalled) = (Vec::new(), Vec::new());
    for (name, seconds) in
        [("movie-a.bin", 30), ("movie-d.bin", 10), ("movie-e.bin", 10), ("movie-f.bin", 10)]
    {
        let start = Instant::now();
        let (reader, label) = player::open(&engine, name).await;
        let took = start.elapsed();
        let played = player::play(&reader, label, 0, seconds).await;
        opens.push(took + played.first_byte);
        stalled.push(played.stalled);
    }
    limit("open to first byte", &opens, None, OPEN);
    limit("movie and marathon stalled", &stalled, None, Duration::ZERO);
    player::no_leaks(&engine).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_read_as_fast_as_the_link_allows_then_again() {
    let Some(engine) = player::engine().await else { return };
    let (reader, label) = player::open(&engine, "movie-b.bin").await;
    let (start, size) = (Instant::now(), reader.size().min(128 << 20));
    let mut offset = 0;
    while offset < size {
        let data = reader.read_at(offset, 1 << 20).await.unwrap();
        check(label, offset, &data);
        offset += data.len() as u64;
    }
    report(
        "read rate",
        format!(
            "{:.1} MB/s over {} MiB",
            size as f64 / 1e6 / start.elapsed().as_secs_f64(),
            size >> 20
        ),
    );
    drop(reader);
    let (reader, _) = player::open(&engine, "movie-b.bin").await;
    let mut again = Vec::new();
    for offset in (0..size).step_by(CHUNK as usize * 64) {
        let start = Instant::now();
        assert_ne!(
            player::source(&reader, offset).await,
            Source::Network,
            "read again at {offset} went to the network"
        );
        again.push(start.elapsed());
    }
    limit("read again (cache)", &again, None, nfs_limits::SEEN);
    drop(reader);
    player::no_leaks(&engine).await;
}
